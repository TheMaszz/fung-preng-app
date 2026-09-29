// src-tauri/src/stream_player.rs
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
        let output = self.b0 * input
            + self.b1 * self.x1
            + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }
}

#[derive(Clone)]
struct EqProcessor {
    enabled: bool,
    gains: [f32; 7],
    filters: [EqFilter; 7],
}

impl EqProcessor {
    fn new(sample_rate: u32, enabled: bool, gains: [f32; 7]) -> Self {
        let filters = std::array::from_fn(|index| EqFilter::new(sample_rate, EQ_FREQUENCIES[index], gains[index]));
        Self {
            enabled,
            gains,
            filters,
        }
    }

    fn process_sample(&mut self, sample: f32) -> f32 {
        if !self.enabled {
            return sample;
        }

        let mut output = sample;
        for filter in &mut self.filters {
            output = filter.process(output);
        }
        output
    }
}

/// A small stateful linear resampler for converting a stream of interleaved
/// stereo PCM chunks from one sample rate to another, one packet at a time.
/// It carries leftover input frames and fractional read position across
/// calls so there's no discontinuity/click at packet boundaries - this is
/// what `play_stream` was missing: it decoded audio at the track's native
/// rate but fed it straight to the output device's ring buffer as if it
/// were already at the device's rate, which is what caused the pitch-up
/// ("chipmunk") effect.
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

    /// Feed one chunk of interleaved input samples, get back as many
    /// resampled output frames as can currently be produced. Any input
    /// frames that can't yet be fully interpolated are retained internally
    /// and used on the next call.
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
            return Vec::new(); // not enough data yet to interpolate
        }

        let step = self.in_rate as f64 / self.out_rate as f64;
        let mut output = Vec::new();

        loop {
            let idx0 = self.next_src_frame.floor() as usize;
            let idx1 = idx0 + 1;
            if idx1 >= total_frames {
                break; // need more input before we can continue
            }
            let frac = (self.next_src_frame - idx0 as f64) as f32;
            for ch in 0..self.channels {
                let s0 = self.input_buffer[idx0 * self.channels + ch];
                let s1 = self.input_buffer[idx1 * self.channels + ch];
                output.push(s0 + (s1 - s0) * frac);
            }
            self.next_src_frame += step;
        }

        // Drop the input frames we've fully consumed, keeping the rest
        // (plus the fractional offset, shifted to be relative to the new
        // buffer start) for the next call.
        let consumed_frames = self.next_src_frame.floor() as usize;
        let keep_from_frame = consumed_frames.min(total_frames.saturating_sub(1));
        self.next_src_frame -= keep_from_frame as f64;
        self.input_buffer.drain(0..keep_from_frame * self.channels);

        output
    }
}

pub fn play_stream(
    reader: impl std::io::Read + Send + Sync + 'static,
    cancel_flag: Arc<AtomicBool>,
    volume: Arc<AtomicU32>,
    progress_secs: Arc<AtomicU32>,
    is_paused: Arc<AtomicBool>,
    eq_enabled: Arc<AtomicBool>,
    eq_gains: Arc<Mutex<[f32; 7]>>,
    app_handle: AppHandle,
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

    let mut decoder = symphonia::default::get_codecs().make(
        &track.codec_params,
        &DecoderOptions::default(),
    )?;

    // 2. Setup CPAL Audio Output
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("No audio output device found")?;
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

    // Counts frames actually pulled by the audio device callback (i.e.
    // genuinely audible), as opposed to frames merely decoded/pushed ahead
    // of time into the ring buffer. Reporting progress from here instead of
    // from the decode loop is what keeps the crossfade handoff lined up
    // with what's actually coming out of the speakers - otherwise the
    // transition fires a couple hundred ms "early" relative to what's
    // audible, clipping the tail of the track.
    let mut consumed_frames: u64 = 0;

    let mut eq = EqProcessor::new(
        out_sample_rate,
        eq_enabled.load(Ordering::Relaxed),
        *eq_gains.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
    );

    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _| {
            let current_vol = get_atomic_f32(&vol_clone);
            let paused = pause_clone.load(Ordering::Relaxed);
            let enabled = eq_enabled_clone.load(Ordering::Relaxed);
            let gains = *eq_gains_clone
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if eq.enabled != enabled || eq.gains != gains {
                eq = EqProcessor::new(out_sample_rate, enabled, gains);
            }

            for sample in data.iter_mut() {
                if !paused {
                    if let Ok(s) = consumer.pop() {
                        let processed = eq.process_sample(s * current_vol);
                        *sample = processed;
                    } else {
                        *sample = 0.0; // Underrun buffer silence
                    }
                } else {
                    *sample = 0.0; // Immediate silence when paused
                }
            }

            if !paused {
                consumed_frames += (data.len() / 2) as u64; // output is forced stereo
                let secs = (consumed_frames / out_sample_rate as u64) as u32;
                progress_clone.store(secs, Ordering::Relaxed);
            }
        },
        move |err| eprintln!("Audio Stream Error: {}", err),
        None,
    )?;

    stream.play()?;

    // Converts each decoded packet from the track's native sample rate to
    // whatever the device is actually running at - without this, audio
    // decoded at (say) 48kHz gets played out through a 44.1kHz clock,
    // which plays it faster than intended and shifts the pitch up.
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
                // dropped mid-stream. This still ends playback (recovering
                // by reopening the HTTP range request from here is a
                // bigger change), but at minimum it must be visible in the
                // logs as an error, not silently look identical to the
                // track finishing normally - a silent stop here is exactly
                // what makes a song "end" while it clearly still had time
                // left.
                eprintln!("[STREAM] Playback stopped early, likely a network error: {e}");
                break;
            }
        };

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(_) => continue,
        };

        let mut sample_buf = SampleBuffer::<f32>::new(
            decoded.capacity() as u64,
            *decoded.spec(),
        );
        sample_buf.copy_interleaved_ref(decoded);
        let samples = sample_buf.samples();

        // Process audio channels & push to RingBuffer
        let mut stereo_chunk = Vec::with_capacity((samples.len() / channels.max(1)) * 2);
        let mut i = 0;
        while i < samples.len() {
            let (left, right) = match channels {
                1 => (samples[i], samples[i]), // Mono -> Stereo Duplicate
                2 => (samples[i], samples[i + 1]), // Standard Stereo
                _ => (samples[i], samples[i + 1]), // Multi-channel downmix
            };
            stereo_chunk.push(left);
            stereo_chunk.push(right);
            i += channels;
        }

        // Convert this chunk to the device's actual output sample rate
        // before pushing it out - see StreamResampler's docs above for why.
        let resampled_chunk = resampler.process(&stereo_chunk);

        let mut j = 0;
        while j < resampled_chunk.len() {
            while producer.is_full() {
                if cancel_flag.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
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

    let mut decoder = symphonia::default::get_codecs().make(
        &track.codec_params,
        &DecoderOptions::default(),
    )?;

    let mut full_pcm = Vec::new();
    let mut packet_count: usize = 0;

    loop {
        match format.next_packet() {
            Ok(packet) => {
                packet_count += 1;
                if let Ok(decoded) = decoder.decode(&packet) {
                    let mut sample_buf = SampleBuffer::<f32>::new(
                        decoded.capacity() as u64,
                        *decoded.spec(),
                    );
                    sample_buf.copy_interleaved_ref(decoded);
                    full_pcm.extend_from_slice(sample_buf.samples());
                }
            }
            // A genuine, clean end of stream - symphonia signals this as
            // an IoError wrapping UnexpectedEof. This is the normal,
            // expected way decoding finishes.
            Err(symphonia::core::errors::Error::IoError(ref io_err))
                if io_err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            // Anything else - a dropped network connection, a reset
            // stream, a corrupted packet - is NOT the same as reaching the
            // real end of the track, even though from here it looks
            // identical (next_packet() just stops returning Ok). Silently
            // treating it as EOF is what truncates a track to however far
            // it got before the hiccup; that truncated length then gets
            // used downstream as this track's *true* duration (crossfade
            // placement, "remaining time" math), which is what causes a
            // transition to fire far too early relative to the real song.
            // Surface it as a real failure instead of pretending it's fine.
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
/// (out_rate, out_channels).
///
/// This exists because the mixed crossfade buffer is produced at whatever
/// sample rate/channel count the *source track* happened to be encoded at
/// (e.g. 48kHz from a YouTube opus stream), but cpal's `build_output_stream`
/// requires a config the *output device* actually supports. `play_stream`
/// sidesteps this by always negotiating against the device's own
/// `default_output_config()` sample rate and forcing stereo - this does the
/// same conversion for a pre-mixed buffer instead of a live decode.
///
/// Uses linear interpolation for resampling and simple duplication/mixdown
/// for channel conversion - good enough for realtime playback, not intended
/// to be a mastering-quality resampler.
fn prepare_for_output(
    samples: Vec<f32>,
    in_rate: u32,
    in_channels: usize,
    out_rate: u32,
    out_channels: usize,
) -> Vec<f32> {
    let in_channels = in_channels.max(1);

    if in_channels == 0 || samples.is_empty() {
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
                    // Downmix anything wider than stereo to L/R using the
                    // first two channels (mirrors play_stream's approach).
                    out.push(samples[base]);
                    out.push(samples[base + 1]);
                }
                1 => {
                    // Average all input channels down to mono.
                    let sum: f32 = samples[base..base + in_channels].iter().sum();
                    out.push(sum / in_channels as f32);
                }
                _ => {
                    // Uncommon target channel counts: take/pad as needed.
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

pub fn play_pcm_buffer(
    samples: Vec<f32>,
    sample_rate: u32,
    channels: usize,
    initial_offset_secs: f64,
    cancel_flag: Arc<AtomicBool>,
    volume: Arc<AtomicU32>,
    progress_secs: Arc<AtomicU32>,
    is_paused: Arc<AtomicBool>,
    eq_enabled: Arc<AtomicBool>,
    eq_gains: Arc<Mutex<[f32; 7]>>,
    app_handle: AppHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("No audio output device found")?;
    let default_config = device.default_output_config()?;

    // Negotiate the same way play_stream does: use the device's actual
    // output sample rate and force stereo, instead of assuming the mixed
    // buffer's native rate/channels are directly supported.
    let out_sample_rate = default_config.sample_rate().0;
    let out_channels: usize = 2;

    let samples = prepare_for_output(samples, sample_rate, channels, out_sample_rate, out_channels);
    let channels = out_channels;

    let config = cpal::StreamConfig {
        channels: channels as u16,
        sample_rate: cpal::SampleRate(out_sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let (mut producer, mut consumer) = rtrb::RingBuffer::<f32>::new(32768);

    let vol_clone = Arc::clone(&volume);
    let pause_clone = Arc::clone(&is_paused);
    let progress_clone = Arc::clone(&progress_secs);
    let eq_enabled_clone = Arc::clone(&eq_enabled);
    let eq_gains_clone = Arc::clone(&eq_gains);

    // Report progress from the audio callback (frames actually consumed by
    // the device), offset by where this buffer starts in Track B's own
    // true timeline - NOT frames merely pushed ahead of time, and NOT
    // reset to 0. This is what lets a transition_at_secs computed from a
    // fresh decode of this same track (always measured from true position
    // 0) line up correctly with what's actually playing, instead of the
    // two clocks silently disagreeing.
    let mut consumed_frames: u64 = 0;

    let mut eq = EqProcessor::new(
        out_sample_rate,
        eq_enabled_clone.load(Ordering::Relaxed),
        *eq_gains_clone.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
    );

    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _| {
            let current_vol = get_atomic_f32(&vol_clone);
            let paused = pause_clone.load(Ordering::Relaxed);
            let enabled = eq_enabled_clone.load(Ordering::Relaxed);
            let gains = *eq_gains_clone
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if eq.enabled != enabled || eq.gains != gains {
                eq = EqProcessor::new(out_sample_rate, enabled, gains);
            }

            for sample in data.iter_mut() {
                if !paused {
                    if let Ok(s) = consumer.pop() {
                        let processed = eq.process_sample(s * current_vol);
                        *sample = processed;
                    } else {
                        *sample = 0.0;
                    }
                } else {
                    *sample = 0.0;
                }
            }

            if !paused {
                consumed_frames += (data.len() / 2) as u64;
                let elapsed_secs = consumed_frames as f64 / out_sample_rate as f64;
                let secs = (initial_offset_secs + elapsed_secs) as u32;
                progress_clone.store(secs, Ordering::Relaxed);
            }
        },
        move |err| eprintln!("Audio Stream Error: {}", err),
        None,
    )?;

    stream.play()?;

    let mut sample_index = 0;
    let total_samples = samples.len();

    // Push pre-mixed buffer frames into CPAL RingBuffer
    while sample_index < total_samples && !cancel_flag.load(Ordering::SeqCst) {
        while is_paused.load(Ordering::Relaxed) {
            if cancel_flag.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        while producer.is_full() {
            if cancel_flag.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        if let Some(&s) = samples.get(sample_index) {
            let _ = producer.push(s);
            sample_index += 1;
        }
    }

    if !cancel_flag.load(Ordering::SeqCst) {
        let _ = app_handle.emit("track-ended", ());
    }

    Ok(())
}