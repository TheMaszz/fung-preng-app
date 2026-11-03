// src-tauri/src/stream_player.rs
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSourceStream, ReadOnlySource};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tauri::{AppHandle, Emitter};

pub fn get_atomic_f32(atomic: &AtomicU32) -> f32 {
    f32::from_bits(atomic.load(Ordering::Relaxed))
}

const EQ_FREQUENCIES: [f32; 7] = [60.0, 150.0, 400.0, 1000.0, 2400.0, 6000.0, 15000.0];

// ---------------------------------------------------------------------------
// EQ
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct EqFilter {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl EqFilter {
    /// Peaking EQ biquad (RBJ cookbook).
    fn new(sample_rate: u32, center_hz: f32, gain_db: f32) -> Self {
        let freq = center_hz.max(20.0);
        let q = 1.2f32;
        let omega = 2.0 * std::f32::consts::PI * freq / sample_rate as f32;
        let sin_omega = omega.sin();
        let cos_omega = omega.cos();
        let a = 10.0_f32.powf(gain_db / 40.0);
        let alpha = sin_omega / (2.0 * q);
        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_omega;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_omega;
        let a2 = 1.0 - alpha / a;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.b0 * input + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }

    /// Update coefficients only; keep x1/x2/y1/y2 so there's no click.
    fn set_gain(&mut self, sample_rate: u32, center_hz: f32, gain_db: f32) {
        let n = Self::new(sample_rate, center_hz, gain_db);
        self.b0 = n.b0;
        self.b1 = n.b1;
        self.b2 = n.b2;
        self.a1 = n.a1;
        self.a2 = n.a2;
    }
}

struct EqProcessor {
    sample_rate: u32,
    enabled: bool,
    gains: [f32; 7],
    pre_gain: f32,
    /// [channel][band] - left and right each have their own filter state.
    filters: [[EqFilter; 7]; 2],
}

impl EqProcessor {
    fn new(sample_rate: u32, enabled: bool, gains: [f32; 7]) -> Self {
        let make =
            || std::array::from_fn(|i| EqFilter::new(sample_rate, EQ_FREQUENCIES[i], gains[i]));
        Self {
            sample_rate,
            enabled,
            gains,
            pre_gain: Self::pre_gain_for(&gains),
            filters: [make(), make()],
        }
    }

    /// Headroom equal to the biggest boost, so boosted peaks don't clip.
    fn pre_gain_for(gains: &[f32; 7]) -> f32 {
        let max_boost = gains.iter().cloned().fold(0.0_f32, f32::max);
        10.0_f32.powf(-max_boost / 20.0)
    }

    fn update(&mut self, enabled: bool, gains: [f32; 7]) {
        self.enabled = enabled;
        if gains != self.gains {
            for band in 0..7 {
                if gains[band] != self.gains[band] {
                    for ch in 0..2 {
                        self.filters[ch][band].set_gain(
                            self.sample_rate,
                            EQ_FREQUENCIES[band],
                            gains[band],
                        );
                    }
                }
            }
            self.gains = gains;
            self.pre_gain = Self::pre_gain_for(&gains);
        }
    }

    fn process_frame(&mut self, mut l: f32, mut r: f32) -> (f32, f32) {
        if !self.enabled {
            return (l, r);
        }
        l *= self.pre_gain;
        r *= self.pre_gain;
        for band in 0..7 {
            l = self.filters[0][band].process(l);
            r = self.filters[1][band].process(r);
        }
        (l, r)
    }
}

/// Fills one audio callback buffer from the ring buffer, applying volume,
/// EQ and a final clamp. Reads samples in L/R pairs only, so an underrun
/// can never leave the channels swapped.
fn fill_output(
    data: &mut [f32],
    consumer: &mut rtrb::Consumer<f32>,
    eq: &mut EqProcessor,
    volume: f32,
    paused: bool,
    stopped: bool,
) {
    for frame in data.chunks_mut(2) {
        if stopped || paused || consumer.slots() < 2 {
            frame.fill(0.0); // stopped, paused or underrun: silence
            continue;
        }
        let l = consumer.pop().unwrap_or(0.0);
        let r = consumer.pop().unwrap_or(0.0);
        let (l, r) = eq.process_frame(l * volume, r * volume);
        frame[0] = l.clamp(-1.0, 1.0);
        if frame.len() > 1 {
            frame[1] = r.clamp(-1.0, 1.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Streaming resampler
// ---------------------------------------------------------------------------

/// A small stateful linear resampler for converting a stream of interleaved
/// stereo PCM chunks from one sample rate to another, one packet at a time.
/// It carries leftover input frames and fractional read position across
/// calls so there's no discontinuity/click at packet boundaries.
struct StreamResampler {
    in_rate: u32,
    out_rate: u32,
    channels: usize,
    input_buffer: Vec<f32>,
    next_src_frame: f64,
}

impl StreamResampler {
    fn new(in_rate: u32, out_rate: u32, channels: usize) -> Self {
        Self {
            in_rate,
            out_rate,
            channels,
            input_buffer: Vec::new(),
            next_src_frame: 0.0,
        }
    }

    fn process(&mut self, input: &[f32]) -> Vec<f32> {
        if self.channels == 0 {
            return Vec::new();
        }
        if self.in_rate == self.out_rate {
            return input.to_vec();
        }

        self.input_buffer.extend_from_slice(input);
        let total_frames = self.input_buffer.len() / self.channels;
        if total_frames < 2 {
            return Vec::new();
        }

        let step = self.in_rate as f64 / self.out_rate as f64;
        let mut output = Vec::new();

        loop {
            let idx0 = self.next_src_frame.floor() as usize;
            let idx1 = idx0 + 1;
            if idx1 >= total_frames {
                break;
            }
            let frac = (self.next_src_frame - idx0 as f64) as f32;
            for ch in 0..self.channels {
                let s0 = self.input_buffer[idx0 * self.channels + ch];
                let s1 = self.input_buffer[idx1 * self.channels + ch];
                output.push(s0 + (s1 - s0) * frac);
            }
            self.next_src_frame += step;
        }

        let consumed_frames = self.next_src_frame.floor() as usize;
        let keep_from_frame = consumed_frames.min(total_frames.saturating_sub(1));
        self.next_src_frame -= keep_from_frame as f64;
        self.input_buffer.drain(0..keep_from_frame * self.channels);

        output
    }
}

// ---------------------------------------------------------------------------
// Live streaming playback
// ---------------------------------------------------------------------------

pub fn play_stream(
    reader: impl std::io::Read + Send + Sync + 'static,
    cancel_flag: Arc<AtomicBool>,
    volume: Arc<AtomicU32>,
    progress_secs: Arc<AtomicU64>,
    is_paused: Arc<AtomicBool>,
    eq_enabled: Arc<AtomicBool>,
    eq_gains: Arc<Mutex<[f32; 7]>>,
    app_handle: AppHandle,
    progress_epoch: Arc<AtomicU64>,
    my_epoch: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Setup Symphonia decoder with ReadOnlySource wrapper
    let read_only_source = ReadOnlySource::new(reader);
    let mss = MediaSourceStream::new(Box::new(read_only_source), Default::default());

    let mut hint = Hint::new();
    hint.with_extension("webm");

    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;
    let track = format.default_track().ok_or("No default track found")?;

    let track_sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);

    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    // 2. Setup CPAL Audio Output
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("No audio output device found")?;
    let default_config = device.default_output_config()?;

    let out_sample_rate = default_config.sample_rate().0;
    let config = cpal::StreamConfig {
        channels: 2, // Force Stereo Output
        sample_rate: cpal::SampleRate(out_sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    // 3. RingBuffer for sending PCM samples to CPAL audio thread
    let (mut producer, mut consumer) = rtrb::RingBuffer::<f32>::new(32768);

    let vol_clone = Arc::clone(&volume);
    let pause_clone = Arc::clone(&is_paused);
    let progress_clone = Arc::clone(&progress_secs);
    let eq_enabled_clone = Arc::clone(&eq_enabled);
    let eq_gains_clone = Arc::clone(&eq_gains);
    let epoch_clone = Arc::clone(&progress_epoch);
    let callback_cancel_flag = Arc::clone(&cancel_flag);

    // Counts frames actually pulled by the audio device callback (i.e.
    // genuinely audible), not frames merely decoded/pushed ahead of time.
    let mut consumed_frames: u64 = 0;

    let mut eq = EqProcessor::new(
        out_sample_rate,
        eq_enabled.load(Ordering::Relaxed),
        *eq_gains
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    );

    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _| {
            let current_vol = get_atomic_f32(&vol_clone);
            let paused = pause_clone.load(Ordering::Relaxed);
            let enabled = eq_enabled_clone.load(Ordering::Relaxed);

            // Never block the audio thread on a mutex.
            match eq_gains_clone.try_lock() {
                Ok(g) => eq.update(enabled, *g),
                Err(_) => {
                    let g = eq.gains;
                    eq.update(enabled, g);
                }
            }

            fill_output(
                data,
                &mut consumer,
                &mut eq,
                current_vol,
                paused,
                callback_cancel_flag.load(Ordering::Relaxed),
            );

            if !paused {
                consumed_frames += (data.len() / 2) as u64; // output is forced stereo
                // Only the newest stream may report progress. During a
                // handoff the old stream keeps playing briefly; without
                // this check it would overwrite the new track's position
                // with its own (e.g. 241s) and trigger the next transition.
                if epoch_clone.load(Ordering::Relaxed) == my_epoch {
                    let secs = consumed_frames as f64 / out_sample_rate as f64;
                    progress_clone.store(secs.to_bits(), Ordering::Relaxed);
                }
            }
        },
        move |err| eprintln!("Audio Stream Error: {}", err),
        None,
    )?;

    stream.play()?;

    // Converts each decoded packet from the track's native sample rate to
    // whatever the device is actually running at.
    let mut resampler = StreamResampler::new(track_sample_rate, out_sample_rate, 2);

    // 4. Decoding Loop
    while !cancel_flag.load(Ordering::SeqCst) {
        // Sleep loop when paused to minimize CPU consumption
        while is_paused.load(Ordering::Relaxed) {
            if cancel_flag.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        if cancel_flag.load(Ordering::SeqCst) {
            break;
        }

        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref io_err))
                if io_err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break; // Genuine, clean end of stream.
            }
            Err(e) => {
                // Not a clean EOF - most likely the network connection
                // dropped mid-stream. Log it so it doesn't look like the
                // track finishing normally.
                eprintln!("[STREAM] Playback stopped early, likely a network error: {e}");
                break;
            }
        };

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let mut sample_buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        sample_buf.copy_interleaved_ref(decoded);
        let samples = sample_buf.samples();

        // Process audio channels & push to RingBuffer
        let ch = channels.max(1);
        let mut stereo_chunk = Vec::with_capacity((samples.len() / ch) * 2);
        let mut i = 0;
        while i + ch <= samples.len() {
            let (left, right) = if ch == 1 {
                (samples[i], samples[i]) // Mono -> Stereo duplicate
            } else {
                (samples[i], samples[i + 1]) // Stereo / multi-channel: first two
            };
            stereo_chunk.push(left);
            stereo_chunk.push(right);
            i += ch;
        }

        // Convert this chunk to the device's actual output sample rate.
        let resampled_chunk = resampler.process(&stereo_chunk);

        let mut j = 0;
        while j + 1 < resampled_chunk.len() {
            // Always wait for room for a full L/R pair so channels never
            // get out of step.
            while producer.slots() < 2 {
                if cancel_flag.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            if cancel_flag.load(Ordering::SeqCst) {
                break;
            }

            let _ = producer.push(resampled_chunk[j]);
            let _ = producer.push(resampled_chunk[j + 1]);

            j += 2;
        }
    }

    // Notify frontend when track ends naturally
    if !cancel_flag.load(Ordering::SeqCst) {
        let _ = app_handle.emit("track-ended", ());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Full-track decode (used by the mixer)
// ---------------------------------------------------------------------------

pub fn decode_full_track(
    reader: impl std::io::Read + Send + Sync + 'static,
) -> Result<(Vec<f32>, u32, usize), Box<dyn std::error::Error>> {
    let read_only_source = ReadOnlySource::new(reader);
    let mss = MediaSourceStream::new(Box::new(read_only_source), Default::default());

    let mut hint = Hint::new();
    hint.with_extension("webm");

    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;
    let track = format.default_track().ok_or("No default track found")?;

    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);

    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let mut full_pcm = Vec::new();
    let mut packet_count: usize = 0;

    loop {
        match format.next_packet() {
            Ok(packet) => {
                packet_count += 1;
                if let Ok(decoded) = decoder.decode(&packet) {
                    let mut sample_buf =
                        SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
                    sample_buf.copy_interleaved_ref(decoded);
                    full_pcm.extend_from_slice(sample_buf.samples());
                }
            }
            // A genuine, clean end of stream.
            Err(symphonia::core::errors::Error::IoError(ref io_err))
                if io_err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            // Anything else (dropped connection, reset stream, corrupt
            // packet) is NOT the real end of the track. Surface it as a
            // failure instead of silently truncating the track.
            Err(e) => {
                let decoded_secs =
                    full_pcm.len() as f64 / (sample_rate.max(1) as f64 * channels.max(1) as f64);
                return Err(format!(
                    "Decoding stopped early after {packet_count} packets (~{decoded_secs:.2}s of audio decoded) - this looks like a stream/network error, not the real end of the track: {e}"
                )
                .into());
            }
        }
    }

    Ok((full_pcm, sample_rate, channels))
}

/// Converts an interleaved PCM buffer from (in_rate, in_channels) to
/// (out_rate, out_channels), so a pre-mixed buffer can be played through
/// whatever config the output device actually supports.
///
/// Linear interpolation for resampling, simple duplication/mixdown for
/// channel conversion - fine for realtime playback, not mastering quality.
fn prepare_for_output(
    samples: Vec<f32>,
    in_rate: u32,
    in_channels: usize,
    out_rate: u32,
    out_channels: usize,
) -> Vec<f32> {
    let in_channels = in_channels.max(1);

    if samples.is_empty() {
        return samples;
    }

    // 1. Channel conversion first, at the original sample rate.
    let channel_converted: Vec<f32> = if in_channels == out_channels {
        samples
    } else {
        let frame_count = samples.len() / in_channels;
        let mut out = Vec::with_capacity(frame_count * out_channels);
        for frame in 0..frame_count {
            let base = frame * in_channels;
            match out_channels {
                2 if in_channels == 1 => {
                    let s = samples[base];
                    out.push(s);
                    out.push(s);
                }
                2 => {
                    // Downmix anything wider than stereo using the first
                    // two channels (mirrors play_stream's approach).
                    out.push(samples[base]);
                    out.push(samples[base + 1]);
                }
                1 => {
                    let sum: f32 = samples[base..base + in_channels].iter().sum();
                    out.push(sum / in_channels as f32);
                }
                _ => {
                    for ch in 0..out_channels {
                        out.push(*samples.get(base + ch).unwrap_or(&0.0));
                    }
                }
            }
        }
        out
    };

    // 2. Sample-rate conversion via linear interpolation.
    if in_rate == out_rate || in_rate == 0 || out_channels == 0 {
        return channel_converted;
    }

    let in_frames = channel_converted.len() / out_channels;
    if in_frames == 0 {
        return channel_converted;
    }

    let ratio = out_rate as f64 / in_rate as f64;
    let out_frames = ((in_frames as f64) * ratio).round().max(1.0) as usize;
    let mut resampled = Vec::with_capacity(out_frames * out_channels);

    for out_frame in 0..out_frames {
        let src_pos = out_frame as f64 / ratio;
        let src_index = src_pos.floor() as usize;
        let frac = (src_pos - src_index as f64) as f32;

        let idx0 = src_index.min(in_frames - 1);
        let idx1 = (src_index + 1).min(in_frames - 1);

        for ch in 0..out_channels {
            let s0 = channel_converted[idx0 * out_channels + ch];
            let s1 = channel_converted[idx1 * out_channels + ch];
            resampled.push(s0 + (s1 - s0) * frac);
        }
    }

    resampled
}

// ---------------------------------------------------------------------------
// Pre-mixed buffer playback
// ---------------------------------------------------------------------------

pub fn play_pcm_buffer(
    samples: Vec<f32>,
    sample_rate: u32,
    channels: usize,
    initial_offset_secs: f64,
    start_sample_index: usize,
    cancel_flag: Arc<AtomicBool>,
    volume: Arc<AtomicU32>,
    progress_secs: Arc<AtomicU64>,
    is_paused: Arc<AtomicBool>,
    eq_enabled: Arc<AtomicBool>,
    eq_gains: Arc<Mutex<[f32; 7]>>,
    app_handle: AppHandle,
    progress_epoch: Arc<AtomicU64>,
    my_epoch: u64,
    handoff_cancel: Option<Arc<AtomicBool>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or("No audio output device found")?;
    let default_config = device.default_output_config()?;

    // Use the device's actual output sample rate and force stereo.
    let out_sample_rate = default_config.sample_rate().0;
    let out_channels: usize = 2;

    let samples = prepare_for_output(
        samples,
        sample_rate,
        channels,
        out_sample_rate,
        out_channels,
    );

    let config = cpal::StreamConfig {
        channels: out_channels as u16,
        sample_rate: cpal::SampleRate(out_sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let (mut producer, mut consumer) = rtrb::RingBuffer::<f32>::new(32768);

    let vol_clone = Arc::clone(&volume);
    let pause_clone = Arc::clone(&is_paused);
    let progress_clone = Arc::clone(&progress_secs);
    let eq_enabled_clone = Arc::clone(&eq_enabled);
    let eq_gains_clone = Arc::clone(&eq_gains);
    let epoch_clone = Arc::clone(&progress_epoch);
    let callback_cancel_flag = Arc::clone(&cancel_flag);

    // Progress is reported from the audio callback, offset by where this
    // buffer starts in the track's own true timeline.
    let mut consumed_frames: u64 = 0;

    let mut eq = EqProcessor::new(
        out_sample_rate,
        eq_enabled_clone.load(Ordering::Relaxed),
        *eq_gains_clone
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    );

    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _| {
            let current_vol = get_atomic_f32(&vol_clone);
            let paused = pause_clone.load(Ordering::Relaxed);
            let enabled = eq_enabled_clone.load(Ordering::Relaxed);

            // Never block the audio thread on a mutex.
            match eq_gains_clone.try_lock() {
                Ok(g) => eq.update(enabled, *g),
                Err(_) => {
                    let g = eq.gains;
                    eq.update(enabled, g);
                }
            }

            fill_output(
                data,
                &mut consumer,
                &mut eq,
                current_vol,
                paused,
                callback_cancel_flag.load(Ordering::Relaxed),
            );

            if !paused {
                consumed_frames += (data.len() / 2) as u64;
                if epoch_clone.load(Ordering::Relaxed) == my_epoch {
                    let elapsed_secs = consumed_frames as f64 / out_sample_rate as f64;
                    let secs = initial_offset_secs + elapsed_secs;
                    progress_clone.store(secs.to_bits(), Ordering::Relaxed);
                }
            }
        },
        move |err| eprintln!("Audio Stream Error: {}", err),
        None,
    )?;

    let total_samples = samples.len();
    let mut sample_index = start_sample_index.min(total_samples) & !1;

    // Prime about 45 ms of output before starting the device. Starting an
    // empty callback first can emit silence during the most audible part of
    // a track handoff.
    let prime_until = total_samples.min(4096);
    while sample_index + 1 < prime_until && producer.slots() >= 2 {
        let _ = producer.push(samples[sample_index]);
        let _ = producer.push(samples[sample_index + 1]);
        sample_index += 2;
    }

    stream.play()?;
    // The new stream is open and has audio queued, so retire the previous
    // stream now. Its callback observes the cancel flag and stops emitting
    // buffered audio immediately.
    if let Some(old_flag) = handoff_cancel {
        old_flag.store(true, Ordering::SeqCst);
    }

    // Push pre-mixed buffer frames into the ring buffer, one L/R pair at a time.
    while sample_index + 1 < total_samples && !cancel_flag.load(Ordering::SeqCst) {
        while is_paused.load(Ordering::Relaxed) {
            if cancel_flag.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        while producer.slots() < 2 {
            if cancel_flag.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        if cancel_flag.load(Ordering::SeqCst) {
            break;
        }

        let _ = producer.push(samples[sample_index]);
        let _ = producer.push(samples[sample_index + 1]);
        sample_index += 2;
    }

    if !cancel_flag.load(Ordering::SeqCst) {
        let _ = app_handle.emit("track-ended", ());
    }

    Ok(())
}
