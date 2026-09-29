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

/// Converts channel count via simple duplication/downmix, independent of any
/// sample-rate change. Mirrors the mono<->stereo handling used elsewhere in
/// the app (see stream_player's prepare_for_output) so two tracks with
/// different channel counts don't get zipped together frame-for-frame with
/// mismatched channel layouts.
pub fn convert_channels(input: &[f32], in_channels: usize, out_channels: usize) -> Vec<f32> {
    if in_channels == out_channels || in_channels == 0 || out_channels == 0 {
        return input.to_vec();
    }
    let frame_count = input.len() / in_channels;
    let mut out = Vec::with_capacity(frame_count * out_channels);
    for frame in 0..frame_count {
        let base = frame * in_channels;
        match out_channels {
            2 if in_channels == 1 => {
                let s = input[base];
                out.push(s);
                out.push(s);
            }
            2 => {
                out.push(input[base]);
                out.push(*input.get(base + 1).unwrap_or(&input[base]));
            }
            1 => {
                let sum: f32 = input[base..base + in_channels].iter().sum();
                out.push(sum / in_channels as f32);
            }
            _ => {
                for ch in 0..out_channels {
                    out.push(*input.get(base + ch).unwrap_or(&0.0));
                }
            }
        }
    }
    out
}

/// Aligns `input` (decoded at `from_rate`/`from_channels`) to
/// `to_rate`/`to_channels` — channel conversion first (at the original
/// rate), then resampling, same order as stream_player's
/// prepare_for_output. Use this on Track B before handing it to
/// `Mixer::mix`, which assumes both tracks already share a single
/// sample_rate/channels.
pub fn align_track(
    input: &[f32],
    from_rate: u32,
    from_channels: usize,
    to_rate: u32,
    to_channels: usize,
) -> Result<Vec<f32>> {
    let channel_converted = convert_channels(input, from_channels, to_channels);
    resample(&channel_converted, to_channels, from_rate, to_rate)
}