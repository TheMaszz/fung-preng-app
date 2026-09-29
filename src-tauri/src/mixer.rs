// src-tauri/src/mixer.rs
use anyhow::Result;
use crate::analyze::AudioAnalysis;

pub struct MixedTrack {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: usize,
    /// The position (in seconds, into Track A's own playback) at which this
    /// buffer is meant to take over. Track A should keep playing normally
    /// right up until it reaches this timestamp, then hand off to `samples`
    /// - which starts exactly at the crossfade, not at Track A's beginning.
    pub transition_at_secs: f64,
    /// The position (in seconds, into Track B's OWN true timeline) at which
    /// this buffer's audio actually begins. Since the buffer skips Track
    /// B's intro, sample 0 of `samples` does NOT correspond to true
    /// position 0 of Track B - it corresponds to this offset. Playback
    /// position must be tracked as `starts_at_secs_in_b + elapsed`, not
    /// reset to 0, otherwise it can never correctly line up with a
    /// transition_at_secs computed from a fresh decode of this same track
    /// (which is always measured from true position 0).
    pub starts_at_secs_in_b: f64,
    /// Track B's true total length in seconds, independent of how much of
    /// it this particular buffer actually contains.
    pub track_b_total_secs: f64,
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
        // Gain calculation for loudness normalization
        let gain_a = 10.0f32.powf((target_lufs - analysis_a.lufs_rms_db) / 20.0);
        let gain_b = 10.0f32.powf((target_lufs - analysis_b.lufs_rms_db) / 20.0);

        let track_a_total_secs = samples_a.len() as f64 / channels as f64 / sample_rate as f64;
        let track_b_total_secs = samples_b.len() as f64 / channels as f64 / sample_rate as f64;

        let desired_crossfade_secs = 6.0;

        // Sanity-check Track A's detected outro point. It should sit
        // somewhere in roughly the back half of the track - if analysis
        // reports something implausibly early (e.g. near the start), trust
        // it less and fall back to "the last few seconds of the track"
        // rather than produce a crossfade window covering most of the song.
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

        // Even a "plausible" outro point can leave too little tail audio to
        // actually fill the desired crossfade window (e.g. outro detected
        // at 180.6s on a 181.5s track = 0.9s of material to fade with).
        // That produces a technically-correct but audibly abrupt "crossfade"
        // that sounds like a hard cut instead of a smooth blend. Pull the
        // outro point back so there's always enough tail room, unless the
        // track itself is shorter than the desired crossfade.
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

        // Sanity-check Track B's detected intro-end point. It should be a
        // short lead-in near the *start* of the track - typically a handful
        // to a few tens of seconds. If analysis reports something
        // implausibly large (e.g. mis-detected near the track's own end),
        // trust it less: clamping intro_sample_b almost up to
        // samples_b.len() would leave almost nothing for "Track B body",
        // producing a mixed buffer that's essentially just the crossfade
        // window and nothing else - which is exactly what would cause a
        // song to appear to last only a few seconds before skipping ahead.
        let max_reasonable_intro_secs = 45.0_f64.min(track_b_total_secs * 0.3);
        let safe_intro_end_sec = if analysis_b.intro_end_sec > max_reasonable_intro_secs
            || analysis_b.intro_end_sec < 0.0
        {
            eprintln!(
                "[MIXER] Track B intro_end_sec ({:.2}s) looks implausible for a {:.2}s track - using full track instead",
                analysis_b.intro_end_sec, track_b_total_secs
            );
            0.0
        } else {
            analysis_b.intro_end_sec
        };

        // Convert timestamps to sample indices
        let outro_sample_a = ((safe_outro_start_sec * sample_rate as f64) as usize * channels)
            .min(samples_a.len());

        let intro_sample_b = ((safe_intro_end_sec * sample_rate as f64) as usize * channels)
            .min(samples_b.len());

        // 6-second smart crossfade window
        let max_crossfade_samples = (desired_crossfade_secs * sample_rate as f64) as usize * channels;
        let crossfade_len = max_crossfade_samples.min(samples_a.len().saturating_sub(outro_sample_a));

        // NOTE: we deliberately do NOT include samples_a[..outro_sample_a]
        // here. Track A already plays normally via the regular streaming
        // path before this buffer is ever handed to the output device -
        // including its body again here would just replay the whole song
        // from the start before reaching the blend. This buffer only needs
        // to cover the handoff itself: the crossfade window, then the rest
        // of Track B.
        let mut output = Vec::with_capacity(
            crossfade_len + samples_b.len().saturating_sub(intro_sample_b),
        );

        // Equal-power crossfade zone
        for i in 0..crossfade_len {
            let t = i as f32 / crossfade_len as f32;
            let fade_out = (1.0 - t).sqrt();
            let fade_in = t.sqrt();

            let sample_a = samples_a.get(outro_sample_a + i).copied().unwrap_or(0.0) * gain_a;
            let sample_b = samples_b.get(intro_sample_b + i).copied().unwrap_or(0.0) * gain_b;

            output.push(sample_a * fade_out + sample_b * fade_in);
        }

        // Track B body (the remainder of B after the part already used in
        // the crossfade zone above)
        let remaining_b_start = intro_sample_b + crossfade_len;
        if remaining_b_start < samples_b.len() {
            for &sample in &samples_b[remaining_b_start..] {
                output.push(sample * gain_b);
            }
        }

        let transition_at_secs = outro_sample_a as f64 / channels as f64 / sample_rate as f64;

        let crossfade_secs = crossfade_len as f64 / channels as f64 / sample_rate as f64;
        let body_secs = (output.len() - crossfade_len).max(0) as f64 / channels as f64 / sample_rate as f64;
        println!(
            "[MIXER] crossfade={:.2}s, Track B body={:.2}s, total mixed buffer={:.2}s (Track B full length was {:.2}s)",
            crossfade_secs,
            body_secs,
            crossfade_secs + body_secs,
            track_b_total_secs
        );

        Ok(MixedTrack {
            samples: output,
            sample_rate,
            channels,
            transition_at_secs,
            starts_at_secs_in_b: safe_intro_end_sec,
            track_b_total_secs,
        })
    }
}