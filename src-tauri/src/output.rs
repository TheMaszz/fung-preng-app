// src-tauri/src/output.rs
//
// Takes decoded PCM (from decode.rs) and plays it out through cpal.
// This is intentionally simple for now — a single track, no mixing, no
// crossfade — just proving the decode -> speaker path works cleanly.
// The mixer (step 8) will replace the "read straight from a Vec" data
// source with something that blends two tracks, but this playback
// mechanism itself stays the same.

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::decode::DecodedAudio;

/// Plays the given audio synchronously — blocks the calling thread until
/// playback finishes. Fine for a test/CLI tool; the real app will want an
/// async/non-blocking version once we get to the mixer, but this is the
/// simplest way to first confirm audio actually comes out of your speakers.
pub fn play_blocking(audio: &DecodedAudio) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow!("no default output device found"))?;

    // Windows (and some other backends) won't accept an arbitrary sample
    // rate/channel combo — the device only supports specific configs it
    // advertises. Look for one that matches our decoded audio exactly;
    // if none exists, we resample to whatever the device does support
    // (see below) so pitch/speed stay correct either way.
    let supported = device.supported_output_configs()?;
    let matching_config = supported
        .filter(|c| c.channels() == audio.channels as u16)
        .find(|c| {
            let range = c.min_sample_rate().0..=c.max_sample_rate().0;
            range.contains(&audio.sample_rate)
        });

    // If the device doesn't natively support our decoded sample rate,
    // resample the audio to whatever rate it does support instead of
    // just playing it at the wrong speed.
    let (config, samples_to_play): (cpal::StreamConfig, Vec<f32>) = match matching_config {
        Some(c) => (
            c.with_sample_rate(cpal::SampleRate(audio.sample_rate)).into(),
            audio.samples.clone(),
        ),
        None => {
            let default = device.default_output_config()?;
            let target_rate = default.sample_rate().0;
            eprintln!(
                "device doesn't support {} Hz natively — resampling to {} Hz",
                audio.sample_rate, target_rate
            );
            let resampled = crate::resample::resample(
                &audio.samples,
                audio.channels,
                audio.sample_rate,
                target_rate,
            )?;
            (default.into(), resampled)
        }
    };

    // Shared read position into the sample buffer. Using an Arc so both this
    // function and the audio callback (which runs on a separate real-time
    // thread cpal manages) can see the same position.
    let samples = Arc::new(samples_to_play);
    let position = Arc::new(AtomicUsize::new(0));
    let position_cb = Arc::clone(&position);
    let samples_cb = Arc::clone(&samples);

    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
            let pos = position_cb.load(Ordering::Relaxed);
            let remaining = samples_cb.len().saturating_sub(pos);
            let to_copy = remaining.min(data.len());

            data[..to_copy].copy_from_slice(&samples_cb[pos..pos + to_copy]);
            // Fill anything past the end of the track with silence rather
            // than leaving garbage/old buffer data in the output.
            if to_copy < data.len() {
                data[to_copy..].fill(0.0);
            }

            position_cb.store(pos + to_copy, Ordering::Relaxed);
        },
        move |err| eprintln!("audio output stream error: {err}"),
        None,
    )?;

    stream.play()?;

    // Block for roughly the track's duration, checking every 200ms so we
    // return promptly once playback actually finishes rather than always
    // waiting the full nominal duration.
    let total_samples = samples.len();
    loop {
        std::thread::sleep(Duration::from_millis(200));
        if position.load(Ordering::Relaxed) >= total_samples {
            break;
        }
    }

    Ok(())
}