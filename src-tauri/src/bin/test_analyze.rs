// src-tauri/src/bin/test_analyze.rs
//
// Pipeline test: extract -> decode -> analyze features.
// Run with: cargo run --bin test_analyze -- <video_id>

use anyhow::Result;
use fung_preng_app_lib::{analyze, decode, extract};

#[tokio::main]
async fn main() ->Result<()> {
    let video_id = std::env::args()
        .nth(1)
        .expect("usage: test_analyze <video_id>");

    println!("Extracting audio for video_id={video_id}...");
    let path = extract::extract_audio(&video_id).await?;

    println!("Decoding...");
    let audio = decode::decode_to_pcm(&path)?;
    println!(
        "Decoded: {} Hz, {} channel(s), {:.1}s",
        audio.sample_rate,
        audio.channels,
        audio.duration_secs()
    );

    println!("Analyzing track features...");
    let analyzer = analyze::Analyzer::new(audio.sample_rate, audio.channels);
    let analysis = analyzer.analyze(&audio.samples)?;

    println!("\n================ Track Analysis ================");
    println!("Duration         : {:.2}s", analysis.duration_secs);
    println!("Loudness (RMS)   : {:.2} dB", analysis.lufs_rms_db);
    println!("Detected BPM     : {:.1}", analysis.bpm);
    println!("Total Beats      : {}", analysis.beat_timestamps.len());
    println!("Intro End Point  : {:.2}s", analysis.intro_end_sec);
    println!("Outro Start Point: {:.2}s", analysis.outro_start_sec);

    if let Some(&first_beat) = analysis.beat_timestamps.first() {
        println!("First Beat At    : {:.3}s", first_beat);
    }
    println!("================================================");

    extract::cleanup(&path).await?;
    println!("Temp file cleaned up.");

    Ok(())
}