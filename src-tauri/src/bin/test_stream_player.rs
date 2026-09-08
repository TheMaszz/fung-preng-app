// src-tauri/src/bin/test_stream_player.rs
//
// The real fix: playback starts almost immediately, decoding continues
// in the background. Compare the time until you actually hear sound
// against test_stream.rs's ~28 second wait.
//
// Run with: cargo run --bin test_stream_player -- <video_id>


use anyhow::Result;
use fung_preng_app_lib::{extract, stream_player};

#[tokio::main]
async fn main() -> Result<()> {
    let video_id = std::env::args()
        .nth(1)
        .expect("usage: test_stream_player <video_id>");

    let start = std::time::Instant::now();

    println!("Resolving direct URL...");
    let direct_url = extract::get_direct_url(&video_id).await?;
    println!("Resolved in {:.2}s — starting playback now", start.elapsed().as_secs_f64());

    stream_player::play_streaming(direct_url, Some("m4a"))?;

    println!("Playback finished at {:.2}s total.", start.elapsed().as_secs_f64());
    Ok(())
}