// src-tauri/src/mixer.rs
use crate::analyze::AudioAnalysis;
use anyhow::Result;

pub struct MixedTrack {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: usize,
    /// The position (in seconds, into Track A's own playback) at which this
    /// buffer is meant to take over. Track A keeps playing normally until it
    /// reaches this timestamp, then hands off to `samples`.
    pub transition_at_secs: f64,
    /// The position (in seconds, into Track B's OWN true timeline) at which
    /// this buffer's audio begins. Playback position of B must be tracked as
    /// `starts_at_secs_in_b + elapsed` - NEVER as `transition_at_secs`
    /// (that is Track A's timeline) and never left at the previous track's
    /// position, or the next transition fires immediately.
    pub starts_at_secs_in_b: f64,
    /// Track B's true total length in seconds.
    pub track_b_total_secs: f64,
    /// The loudness gain that was applied to Track B inside this buffer.
    /// When B later becomes "Track A" of the next mix, pass this value as
    /// `a_playing_gain` to `mix_with_a_gain` so there is no volume jump at
    /// the handoff.
    pub b_gain_applied: f32,
}

pub struct Mixer;

/// Gentle limiter: transparent below 0.9, smoothly compresses peaks above it
/// so the final hard clamp in the audio callback never has to clip.
fn soft_limit(x: f32) -> f32 {
    const THRESHOLD: f32 = 0.9;
    let a = x.abs();
    if a <= THRESHOLD {
        x
    } else {
        x.signum() * (THRESHOLD + (1.0 - THRESHOLD) * ((a - THRESHOLD) / (1.0 - THRESHOLD)).tanh())
    }
}

fn gain_from_lufs(target_lufs: f32, measured_lufs: f32) -> f32 {
    if !measured_lufs.is_finite() || !target_lufs.is_finite() {
        return 1.0;
    }
    10.0f32
        .powf((target_lufs - measured_lufs) / 20.0)
        .clamp(0.25, 4.0)
}

impl Mixer {
    /// Backwards-compatible entry point. Assumes Track A is currently being
    /// heard at unity gain (i.e. it is streamed raw). For chained mixes
    /// (A was itself a pre-mixed buffer) use `mix_with_a_gain` instead.
    pub fn mix(
        samples_a: &[f32],
        analysis_a: &AudioAnalysis,
        samples_b: &[f32],
        analysis_b: &AudioAnalysis,
        sample_rate: u32,
        channels: usize,
        target_lufs: f32,
    ) -> Result<MixedTrack> {
        Self::mix_with_a_gain(
            samples_a,
            analysis_a,
            1.0,
            samples_b,
            analysis_b,
            sample_rate,
            channels,
            target_lufs,
        )
    }

    /// `a_playing_gain` is the gain Track A is *actually being heard at*
    /// right before the handoff:
    ///   - 1.0 if A is streamed raw by `play_stream`
    ///   - the previous MixedTrack's `b_gain_applied` if A is playing from
    ///     a pre-mixed buffer
    pub fn mix_with_a_gain(
        samples_a: &[f32],
        analysis_a: &AudioAnalysis,
        a_playing_gain: f32,
        samples_b: &[f32],
        analysis_b: &AudioAnalysis,
        sample_rate: u32,
        channels: usize,
        target_lufs: f32,
    ) -> Result<MixedTrack> {
        let channels = channels.max(1);

        // Loudness normalisation for B (clamped so odd analysis can't blast).
        let gain_b = gain_from_lufs(target_lufs, analysis_b.lufs_rms_db);

        // A must continue at the level the listener is already hearing,
        // otherwise there's an audible volume step at the handoff.
        let gain_a = if a_playing_gain.is_finite() && a_playing_gain > 0.0 {
            a_playing_gain
        } else {
            1.0
        };

        let track_a_total_secs = samples_a.len() as f64 / channels as f64 / sample_rate as f64;
        let track_b_total_secs = samples_b.len() as f64 / channels as f64 / sample_rate as f64;

        let desired_crossfade_secs = 6.0;

        // --- Track A outro point -------------------------------------------
        let min_reasonable_outro_secs = (track_a_total_secs * 0.5).max(10.0);
        let mut safe_outro_start_sec = if analysis_a.outro_start_sec < min_reasonable_outro_secs
            || analysis_a.outro_start_sec > track_a_total_secs
        {
            let fallback = (track_a_total_secs - desired_crossfade_secs).max(0.0);
            eprintln!(
                "[MIXER] Track A outro_start_sec ({:.2}s) looks implausible for a {:.2}s track - using last {:.2}s instead",
                analysis_a.outro_start_sec, track_a_total_secs, desired_crossfade_secs
            );
            fallback
        } else {
            analysis_a.outro_start_sec
        };

        // Make sure there's enough tail audio to fill the whole crossfade.
        let min_tail_needed = desired_crossfade_secs.min(track_a_total_secs);
        if track_a_total_secs - safe_outro_start_sec < min_tail_needed {
            let adjusted = (track_a_total_secs - min_tail_needed).max(0.0);
            eprintln!(
                "[MIXER] Track A outro_start_sec ({:.2}s) leaves only {:.2}s of tail audio on a {:.2}s track - pulling back to {:.2}s for a full {:.2}s crossfade",
                safe_outro_start_sec,
                track_a_total_secs - safe_outro_start_sec,
                track_a_total_secs,
                adjusted,
                min_tail_needed
            );
            safe_outro_start_sec = adjusted;
        }

        // --- Track B intro point -------------------------------------------
        let max_reasonable_intro_secs = 45.0_f64.min(track_b_total_secs * 0.3);
        let safe_intro_end_sec = if analysis_b.intro_end_sec > max_reasonable_intro_secs
            || analysis_b.intro_end_sec < 0.0
            || !analysis_b.intro_end_sec.is_finite()
        {
            eprintln!(
                "[MIXER] Track B intro_end_sec ({:.2}s) looks implausible for a {:.2}s track - using full track instead",
                analysis_b.intro_end_sec, track_b_total_secs
            );
            0.0
        } else {
            analysis_b.intro_end_sec
        };

        // Start B early enough that its intro plays UNDER A's outro and B's
        // main section arrives just as the crossfade completes. If B has no
        // real intro this is simply 0 (start of the track).
        let b_start_sec = (safe_intro_end_sec - desired_crossfade_secs).max(0.0);

        // --- Convert to sample indices -------------------------------------
        let outro_sample_a = ((safe_outro_start_sec * sample_rate as f64) as usize * channels)
            .min(samples_a.len());
        let start_sample_b =
            ((b_start_sec * sample_rate as f64) as usize * channels).min(samples_b.len());

        let max_crossfade_samples =
            (desired_crossfade_secs * sample_rate as f64) as usize * channels;
        let crossfade_len =
            max_crossfade_samples.min(samples_a.len().saturating_sub(outro_sample_a));

        // The buffer holds only the handoff: the crossfade window followed by
        // the rest of Track B. Track A's body already played via streaming.
        let mut output =
            Vec::with_capacity(crossfade_len + samples_b.len().saturating_sub(start_sample_b));

        // Equal-power crossfade.
        for i in 0..crossfade_len {
            let t = i as f32 / crossfade_len as f32;
            let fade_out = (1.0 - t).sqrt();
            let fade_in = t.sqrt();

            let sample_a = samples_a.get(outro_sample_a + i).copied().unwrap_or(0.0) * gain_a;
            let sample_b = samples_b.get(start_sample_b + i).copied().unwrap_or(0.0) * gain_b;

            output.push(sample_a * fade_out + sample_b * fade_in);
        }

        // Track B body.
        let remaining_b_start = start_sample_b + crossfade_len;
        if remaining_b_start < samples_b.len() {
            for &sample in &samples_b[remaining_b_start..] {
                output.push(sample * gain_b);
            }
        }

        // Soft-limit everything so overlapping/boosted audio never hard-clips.
        for s in output.iter_mut() {
            *s = soft_limit(*s);
        }

        let transition_at_secs = outro_sample_a as f64 / channels as f64 / sample_rate as f64;

        let crossfade_secs = crossfade_len as f64 / channels as f64 / sample_rate as f64;
        let body_secs = output.len().saturating_sub(crossfade_len) as f64
            / channels as f64
            / sample_rate as f64;
        println!(
            "[MIXER] crossfade={:.2}s, Track B body={:.2}s, total mixed buffer={:.2}s (Track B full length was {:.2}s), B starts at {:.2}s in its own timeline, transition at {:.2}s into A",
            crossfade_secs,
            body_secs,
            crossfade_secs + body_secs,
            track_b_total_secs,
            b_start_sec,
            transition_at_secs
        );

        Ok(MixedTrack {
            samples: output,
            sample_rate,
            channels,
            transition_at_secs,
            starts_at_secs_in_b: b_start_sec,
            track_b_total_secs,
            b_gain_applied: gain_b,
        })
    }
}