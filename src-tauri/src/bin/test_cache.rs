// cargo run --bin test_cache

use anyhow::Result;
use fung_preng_app_lib::{analyze, cache};

fn main() -> Result<()> {
    println!("=== Testing Cache Manager ===");
    let cache = cache::CacheManager::default_dir()?;

    let mock_analysis = analyze::AudioAnalysis {
        duration_secs: 210.5,
        lufs_rms_db: -13.5,
        bpm: 128.0,
        beat_timestamps: vec![0.0, 0.468, 0.937],
        intro_end_sec: 12.0,
        outro_start_sec: 195.0,
        energy_profile: vec![0.05, 0.12, 0.45, 0.88],
    };

    let test_id = "demo_cache_video_id";
    println!("Saving analysis to ./cache/{test_id}.json ...");
    cache.save_analysis(test_id, &mock_analysis)?;

    println!("Reading analysis back from disk...");
    if let Some(loaded) = cache.get_analysis(test_id) {
        println!("Cache hit successfully!");
        println!("- Loaded BPM: {}", loaded.bpm);
        println!("- Loaded Loudness: {} dB", loaded.lufs_rms_db);
    } else {
        eprintln!("Cache miss or parse error!");
    }

    Ok(())
}