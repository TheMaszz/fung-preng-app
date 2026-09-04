use anyhow::Result;
use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioAnalysis {
    pub duration_secs: f64,
    pub lufs_rms_db: f32,
    pub bpm: f32,
    pub beat_timestamps: Vec<f64>,
    pub intro_end_sec: f64,
    pub outro_start_sec: f64,
    pub energy_profile: Vec<f32>,
}

pub struct Analyzer {
    sample_rate: u32,
    channels: usize,
}

impl Analyzer {
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        Self { sample_rate, channels }
    }

    pub fn analyze(&self, samples: &[f32]) -> Result<AudioAnalysis> {
        let mono_samples: Vec<f32> = if self.channels > 1 {
            samples
                .chunks(self.channels)
                .map(|chunk| chunk.iter().sum::<f32>() / self.channels as f32)
                .collect()
        } else {
            samples.to_vec()
        };

        let total_samples = mono_samples.len();
        let duration_secs = total_samples as f64 / self.sample_rate as f64;

        let lufs_rms_db = calculate_rms_db(&mono_samples);
        let window_size = (self.sample_rate as usize) / 10; // 100ms
        let energy_profile = calculate_energy_profile(&mono_samples, window_size);

        // Dynamic boundary detection relative to average track loudness
        let (intro_end_sec, outro_start_sec) =
            detect_boundaries(&energy_profile, duration_secs, lufs_rms_db);

        let (bpm, beat_timestamps) =
            estimate_bpm_and_beats(&mono_samples, self.sample_rate)?;

        Ok(AudioAnalysis {
            duration_secs,
            lufs_rms_db,
            bpm,
            beat_timestamps,
            intro_end_sec,
            outro_start_sec,
            energy_profile,
        })
    }
}

fn calculate_rms_db(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return -100.0;
    }
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    let rms = (sum_sq / samples.len() as f32).sqrt();
    if rms > 0.0 { 20.0 * rms.log10() } else { -100.0 }
}

fn calculate_energy_profile(samples: &[f32], window_size: usize) -> Vec<f32> {
    if window_size == 0 { return Vec::new(); }
    samples
        .chunks(window_size)
        .map(|chunk| {
            let sum: f32 = chunk.iter().map(|&s| s.abs()).sum();
            sum / chunk.len() as f32
        })
        .collect()
}

fn detect_boundaries(
    energy: &[f32],
    duration: f64,
    lufs_rms_db: f32,
) -> (f64, f64) {
    if energy.is_empty() {
        return (0.0, duration);
    }

    // Dynamic threshold: 22 dB below the song's average energy level.
    // Clamped between -42 dB and -26 dB to handle ultra-loud or very quiet masters.
    let threshold_db = (lufs_rms_db - 22.0).clamp(-42.0, -26.0);
    let min_linear = 10.0f32.powf(threshold_db / 20.0);

    let chunk_duration = duration / energy.len() as f64;

    // Intro: First chunk exceeding musical energy threshold
    let mut intro_idx = 0;
    while intro_idx < energy.len() && energy[intro_idx] < min_linear {
        intro_idx += 1;
    }

    // Outro: Backward scan with a 1.5-second (15 chunks) moving average window.
    // Ignores short trailing audio, talking, or quiet outro noise.
    let window_len = (1.5 / chunk_duration).max(1.0) as usize;
    let mut outro_idx = energy.len().saturating_sub(1);

    for i in (intro_idx..energy.len()).rev() {
        let start = i.saturating_sub(window_len);
        let avg_energy: f32 = energy[start..=i].iter().sum::<f32>() / (i - start + 1) as f32;
        if avg_energy >= min_linear {
            outro_idx = i;
            break;
        }
    }

    let intro_sec = (intro_idx as f64 * chunk_duration).min(duration);
    let outro_sec = (outro_idx as f64 * chunk_duration).min(duration);

    (intro_sec, outro_sec)
}

fn estimate_bpm_and_beats(samples: &[f32], sample_rate: u32) -> Result<(f32, Vec<f64>)> {
    let fft_size = 1024;
    let hop_size = 512;
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(fft_size);

    let mut scratch = fft.make_scratch_vec();
    let mut input = vec![0.0; fft_size];
    let mut output = fft.make_output_vec();

    let mut prev_magnitudes = vec![0.0; fft_size / 2 + 1];
    let mut onset_envelope = Vec::new();

    for chunk in samples.chunks(hop_size) {
        if chunk.len() < fft_size { break; }
        input.copy_from_slice(&chunk[..fft_size]);

        fft.process_with_scratch(&mut input, &mut output, &mut scratch)?;

        let mut flux = 0.0;
        for (i, bin) in output.iter().enumerate() {
            let mag = bin.norm();
            let diff = mag - prev_magnitudes[i];
            if diff > 0.0 { flux += diff; }
            prev_magnitudes[i] = mag;
        }
        onset_envelope.push(flux);
    }

    let frame_rate = sample_rate as f32 / hop_size as f32;
    let min_bpm = 70.0;
    let max_bpm = 160.0;

    let min_lag = (frame_rate * 60.0 / max_bpm) as usize;
    let max_lag = (frame_rate * 60.0 / min_bpm) as usize;

    let mut max_weighted_corr = 0.0;
    let mut best_lag = min_lag;

    // Autocorrelation with logarithmic tempo-preference weighting (~120 BPM)
    for lag in min_lag..=max_lag.min(onset_envelope.len() / 2) {
        let mut corr = 0.0;
        for i in 0..(onset_envelope.len() - lag) {
            corr += onset_envelope[i] * onset_envelope[i + lag];
        }

        let bpm_candidate = (frame_rate * 60.0) / lag as f32;
        let weight = 1.0 - ((bpm_candidate - 120.0) / 120.0).powi(2).abs() * 0.3;
        let weighted_corr = corr * weight;

        if weighted_corr > max_weighted_corr {
            max_weighted_corr = weighted_corr;
            best_lag = lag;
        }
    }

    let detected_bpm = if best_lag > 0 {
        (frame_rate * 60.0) / best_lag as f32
    } else {
        120.0
    };

    // Phase alignment: Find first prominent onset transient peak
    let mut phase_offset_sec = 0.0;
    let threshold = onset_envelope.iter().cloned().fold(0.0f32, f32::max) * 0.3;
    for (idx, &flux) in onset_envelope.iter().enumerate() {
        if flux > threshold {
            phase_offset_sec = idx as f64 * (hop_size as f64 / sample_rate as f64);
            break;
        }
    }

    let beat_interval = 60.0 / detected_bpm as f64;
    let total_duration = samples.len() as f64 / sample_rate as f64;
    let mut beat_timestamps = Vec::new();
    let mut t = phase_offset_sec;

    while t < total_duration {
        beat_timestamps.push(t);
        t += beat_interval;
    }

    Ok((detected_bpm, beat_timestamps))
}