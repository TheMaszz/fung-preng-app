use anyhow::Result;
use fung_preng_app_lib::{analyze, cache, decode, extract, mixer, output};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        println!("usage: cargo run --bin test_mixer -- <video_id_1> <video_id_2> [--preview]");
        return Ok(());
    }

    let id_a = args[1].clone();
    let id_b = args[2].clone();
    let preview_mode = args.get(3).map(|s| s.as_str()) == Some("--preview");
    let cache = cache::CacheManager::default_dir()?;

    println!("=== 1 & 2. Fetching & Analyzing Tracks A and B in Parallel ===");

    // Run track A and B processing concurrently using tokio::try_join!
    let (track_a, track_b) = tokio::try_join!(
        load_and_analyze(&cache, &id_a),
        load_and_analyze(&cache, &id_b)
    )?;

    let (mut samples_a, mut analysis_a, sample_rate, channels) = track_a;
    let (samples_b, analysis_b, _, _) = track_b;

    // Fast Preview Optimization: Trim Track A to the last 20 seconds before the outro
    if preview_mode {
        println!("\n⚡ PREVIEW MODE ENABLED: Trimming Track A to jump near the transition point...");
        let target_start = (analysis_a.outro_start_sec - 10.0).max(0.0);
        let start_sample = (target_start * sample_rate as f64 * channels as f64) as usize;

        if start_sample < samples_a.len() {
            samples_a = samples_a[start_sample..].to_vec();
            analysis_a.duration_secs -= target_start;
            analysis_a.outro_start_sec -= target_start;
            analysis_a.beat_timestamps = analysis_a
                .beat_timestamps
                .iter()
                .filter_map(|&t| if t >= target_start { Some(t - target_start) } else { None })
                .collect();
        }
    }

    println!("\n=== 3. Mixing Tracks ===");
    let mixed = mixer::Mixer::mix(
        &samples_a,
        &analysis_a,
        &samples_b,
        &analysis_b,
        sample_rate,
        channels,
        -14.0,
    )?;

    println!("\n=== 4. Playing Audio ===");
    let audio_to_play = decode::Audio {
        samples: mixed.samples,
        sample_rate: mixed.sample_rate,
        channels: mixed.channels,
    };

    println!("Playing output (Duration: {:.1}s)...", audio_to_play.duration_secs());
    output::play_blocking(&audio_to_play)?;

    Ok(())
}

async fn load_and_analyze(
    cache: &cache::CacheManager,
    video_id: &str,
) -> Result<(Vec<f32>, analyze::AudioAnalysis, u32, usize)> {
    println!("  -> Starting fetch for {video_id}...");
    let path = extract::extract_audio(video_id).await?;
    let audio = decode::decode_to_pcm(&path)?;

    let analysis = match cache.get_analysis(video_id) {
        Some(cached) => cached,
        None => {
            let analyzer = analyze::Analyzer::new(audio.sample_rate, audio.channels);
            let res = analyzer.analyze(&audio.samples)?;
            let _ = cache.save_analysis(video_id, &res);
            res
        }
    };

    extract::cleanup(&path).await?;
    Ok((audio.samples, analysis, audio.sample_rate, audio.channels))
}