// src-tauri/src/mixer.rs
use crate::analyze::AudioAnalysis;
use anyhow::Result;
use rubato::{FftFixedInOut, Resampler};
use std::f32::consts::PI;

pub struct MixedTrack {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: usize,
}

pub struct Mixer;

impl Mixer {
    pub fn mix(
        samples_a: &[f32],
        analysis_a: &AudioAnalysis,
        samples_b: &[f32],
        analysis_b: &AudioAnalysis,
        sample_rate: u32,
        channels: usize,
        target_lufs: f32,
    ) -> Result<MixedTrack> {
        // 1. Calculate gain adjustments for loudness normalization
        let gain_a = 10.0f32.powf((target_lufs - analysis_a.lufs_rms_db) / 20.0);
        let gain_b = 10.0f32.powf((target_lufs - analysis_b.lufs_rms_db) / 20.0);

        // 2. Determine crossfade duration (8 seconds or 8 bars)
        let fade_duration_sec = 8.0f64;

        // 3. Find target overlap point in Track A
        let target_overlap_sec = (analysis_a.outro_start_sec - fade_duration_sec).max(0.0);

        // 4. Snap overlap to Track A's nearest downbeat
        let overlap_start_sec = analysis_a
            .beat_timestamps
            .iter()
            .copied()
            .find(|&t| t >= target_overlap_sec)
            .unwrap_or(target_overlap_sec);

        let start_sample_a = (overlap_start_sec * sample_rate as f64 * channels as f64) as usize;
        let fade_samples_a = (fade_duration_sec * sample_rate as f64 * channels as f64) as usize;

        let start_sample_a = start_sample_a.min(samples_a.len());
        let fade_samples_a = fade_samples_a.min(samples_a.len() - start_sample_a);

        // 5. BEAT MATCHING: Resample Track B to match Track A's BPM during transition
        let bpm_ratio = (analysis_a.bpm / analysis_b.bpm).clamp(0.85, 1.15);
        let resampled_b = resample_audio(samples_b, channels, bpm_ratio as f64)?;

        let mut output = Vec::new();

        // PART 1: Track A Solo
        for &s in &samples_a[..start_sample_a] {
            output.push((s * gain_a).clamp(-1.0, 1.0));
        }

        // PART 2: Equal-Power Beat-Matched Overlap
        let frame_count = fade_samples_a / channels;
        for frame in 0..frame_count {
            let t = frame as f32 / frame_count as f32;

            // Equal-power crossfade curves
            let w_a = (t * PI / 2.0).cos();
            let w_b = (t * PI / 2.0).sin();

            for ch in 0..channels {
                let idx_a = start_sample_a + frame * channels + ch;
                let idx_b = frame * channels + ch;

                let s_a = samples_a.get(idx_a).copied().unwrap_or(0.0) * gain_a;
                let s_b = resampled_b.get(idx_b).copied().unwrap_or(0.0) * gain_b;

                let mixed_sample = (s_a * w_a) + (s_b * w_b);
                output.push(mixed_sample.clamp(-1.0, 1.0));
            }
        }

        // PART 3: Track B Solo
        let start_sample_b = frame_count * channels;
        if start_sample_b < resampled_b.len() {
            for &s in &resampled_b[start_sample_b..] {
                output.push((s * gain_b).clamp(-1.0, 1.0));
            }
        }

        Ok(MixedTrack {
            samples: output,
            sample_rate,
            channels,
        })
    }
}

/// Helper function to speed up / slow down PCM audio samples using Rubato
fn resample_audio(
    samples: &[f32],
    channels: usize,
    speed_ratio: f64,
) -> Result<Vec<f32>> {
    if (speed_ratio - 1.0).abs() < 0.001 {
        return Ok(samples.to_vec());
    }

    let chunk_size = 1024;
    let mut resampler = FftFixedInOut::<f32>::new(
        10000,
        (10000.0 * speed_ratio) as usize,
        chunk_size,
        channels,
    )?;

    let num_frames = samples.len() / channels;
    let mut channel_buffers = vec![Vec::with_capacity(num_frames); channels];

    // De-interleave channel samples
    for frame in 0..num_frames {
        for ch in 0..channels {
            channel_buffers[ch].push(samples[frame * channels + ch]);
        }
    }

    let mut resampled_channels = vec![Vec::new(); channels];
    let mut pos = 0;

    while pos + chunk_size <= num_frames {
        let chunk: Vec<Vec<f32>> = channel_buffers
            .iter()
            .map(|ch| ch[pos..pos + chunk_size].to_vec())
            .collect();

        let out = resampler.process(&chunk, None)?;
        for ch in 0..channels {
            resampled_channels[ch].extend_from_slice(&out[ch]);
        }
        pos += chunk_size;
    }

    // Re-interleave channel samples
    let out_frames = resampled_channels[0].len();
    let mut interleaved = Vec::with_capacity(out_frames * channels);
    for frame in 0..out_frames {
        for ch in 0..channels {
            interleaved.push(resampled_channels[ch][frame]);
        }
    }

    Ok(interleaved)
}