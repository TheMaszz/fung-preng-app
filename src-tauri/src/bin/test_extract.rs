use anyhow::Result;
use fung_preng_app_lib::{extract};

#[tokio::main]
async fn main() -> Result<()> {
    let video_id = std::env::args()
        .nth(1)
        .expect("usage: test_extract <video_id>");

    println!("Extracting audio for video_id={video_id}...");
    let path = extract::extract_audio(&video_id).await?;

    let metadata = std::fs::metadata(&path)?;
    let size_mb = metadata.len() as f64 / (1024.0 * 1024.0);

    println!("Success: {path:?}");
    println!("File size: {size_mb:.2} MB");
    println!(
        "(a 3-4 min song should be roughly 3-6 MB if this is truly audio-only — \
         if you're seeing 20+ MB, double check the -f bestaudio flag took effect)"
    );

    // Leave the file in place so you can play it manually and confirm it
    // sounds right, before wiring cleanup() into the real pipeline.
    Ok(())
}