// src-tauri/src/bin/test_output.rs
use anyhow::Result;
use fung_preng_app_lib::{decode, extract, output};

#[tokio::main]
async fn main() -> Result<()> {
    let video_id = std::env::args()
        .nth(1)
        .expect("usage: cargo run --bin test_output -- <video_id>");

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

    println!("Playing... (should hear audio now)");
    output::play_blocking(&audio)?;
    println!("Playback finished.");

    extract::cleanup(&path).await?;
    println!("Temp file cleaned up.");

    Ok(())
}