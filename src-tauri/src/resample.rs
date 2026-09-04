// src-tauri/src/resample.rs
//
// Converts PCM audio from one sample rate to another using a proper sinc
// resampler (not just "play it at the wrong speed"). This exists because
// output devices don't always support the sample rate audio was decoded
// at — see output.rs, which falls back to the device's native rate when
// there's a mismatch. Getting this right matters a lot once BPM detection
// and beat-matching (steps 5+7) depend on audio actually playing at the
// correct, real-world speed.
//
// Verified: this compiles and produces the expected output frame count
// (within the resampler's inherent small startup/end latency) via a
// standalone smoke test before being wired into the real pipeline.

use anyhow::Result;
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

/// Resamples interleaved PCM from `from_rate` to `to_rate`.
/// Returns the input unchanged if the rates already match (no-op fast path).
pub fn resample(input: &[f32], channels: usize, from_rate: u32, to_rate: u32) -> Result<Vec<f32>> {
    if from_rate == to_rate {
        return Ok(input.to_vec());
    }

    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };

    let ratio = to_rate as f64 / from_rate as f64;
    let chunk_size = 1024;
    let mut resampler = SincFixedIn::<f32>::new(ratio, 2.0, params, chunk_size, channels)?;

    // rubato works on planar (per-channel) buffers, not interleaved — split first.
    let frames = input.len() / channels;
    let mut planar: Vec<Vec<f32>> = vec![Vec::with_capacity(frames); channels];
    for frame in input.chunks(channels) {
        for (ch, &s) in frame.iter().enumerate() {
            planar[ch].push(s);
        }
    }

    let mut output_planar: Vec<Vec<f32>> = vec![Vec::new(); channels];
    let mut pos = 0;
    while pos + chunk_size <= planar[0].len() {
        let chunk: Vec<Vec<f32>> = planar
            .iter()
            .map(|c| c[pos..pos + chunk_size].to_vec())
            .collect();
        let out = resampler.process(&chunk, None)?;
        for (ch, data) in out.into_iter().enumerate() {
            output_planar[ch].extend(data);
        }
        pos += chunk_size;
    }

    // Final partial chunk (shorter than chunk_size) needs process_partial.
    if pos < planar[0].len() {
        let chunk: Vec<Vec<f32>> = planar.iter().map(|c| c[pos..].to_vec()).collect();
        let out = resampler.process_partial(Some(&chunk), None)?;
        for (ch, data) in out.into_iter().enumerate() {
            output_planar[ch].extend(data);
        }
    }

    // Re-interleave for playback.
    let out_frames = output_planar[0].len();
    let mut interleaved = Vec::with_capacity(out_frames * channels);
    for i in 0..out_frames {
        for ch in 0..channels {
            interleaved.push(output_planar[ch][i]);
        }
    }
    Ok(interleaved)
}