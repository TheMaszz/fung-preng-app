use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::player_state::PlaybackProgress;
use crate::stream_source::HttpRangeSource;

pub fn play_streaming_with_cancel(
    url: String,
    format_hint: Option<&str>,
    cancel_flag: Arc<AtomicBool>,
    is_paused: Arc<AtomicBool>,
    volume: Arc<Mutex<f32>>,
    progress: Arc<Mutex<PlaybackProgress>>,
) -> Result<()> {
    let source = HttpRangeSource::new(url)?;
    let mss = MediaSourceStream::new(Box::new(source), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = format_hint {
        hint.with_extension(ext);
    }

    let format_opts = FormatOptions {
        enable_gapless: true,
        ..Default::default()
    };
    let metadata_opts = MetadataOptions::default();
    let decoder_opts = DecoderOptions::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|e| anyhow!("Failed to probe audio format: {e}"))?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| anyhow!("No default track found in audio source"))?;

    let track_id = track.id;
    let time_base = track.codec_params.time_base;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &decoder_opts)
        .map_err(|e| anyhow!("Failed to create audio decoder: {e}"))?;

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow!("No output audio device found"))?;

    let config = device.default_output_config()?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;

    // Fixed: Using rtrb::RingBuffer
    let (mut producer, mut consumer) = rtrb::RingBuffer::new(sample_rate as usize * 2 * channels);

    let stream = device.build_output_stream(
        &config.into(),
        move |data: &mut [f32], _| {
            for sample in data.iter_mut() {
                *sample = consumer.pop().unwrap_or(0.0);
            }
        },
        |err| eprintln!("Audio output stream error: {err}"),
        None,
    )?;

    stream.play()?;

    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        // 1. Stop playback if flag is set
        if cancel_flag.load(Ordering::SeqCst) {
            break;
        }

        // 2. Pause check: sleep 50ms without decoding
        if is_paused.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(50));
            continue;
        }

        // 3. Read packet and decode
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(_)) => break,
            Err(e) => {
                eprintln!("Decoder error: {e}");
                continue;
            }
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(e) => {
                eprintln!("Packet decode error: {e}");
                continue;
            }
        };

        if sample_buf.is_none() {
            let spec = *decoded.spec();
            let duration = decoded.capacity() as u64;
            sample_buf = Some(SampleBuffer::new(duration, spec));
        }

        if let Some(buf) = &mut sample_buf {
            buf.copy_interleaved_ref(decoded);

            let current_vol = volume.lock().map(|v| *v).unwrap_or(1.0);
            let samples = buf.samples();

            for &sample in samples {
                while producer.is_full() {
                    if cancel_flag.load(Ordering::SeqCst) {
                        return Ok(());
                    }
                    if is_paused.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                let _ = producer.push(sample * current_vol);
            }
        }

        // Update playback position
        if let Some(tb) = time_base {
            let ts = packet.ts();
            let time = tb.calc_time(ts);
            let pos_secs = time.seconds as f64 + time.frac;

            if let Ok(mut prog) = progress.lock() {
                prog.position_secs = pos_secs;
            }
        }
    }

    Ok(())
}