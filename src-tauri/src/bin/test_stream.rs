// src-tauri/src/bin/test_stream.rs
//
// The real fix for the delay: resolves the direct URL (fast), then decodes
// and plays directly from the network via Range requests — no waiting for
// a full file to hit disk first. Compare how much sooner "Playing..." shows
// up here versus test_output.rs's timing.
//
// Run with: cargo run --bin test_stream -- <video_id>

use anyhow::Result;
use fung_preng_app_lib::{extract, decode, stream_source, output};

#[tokio::main]
async fn main() -> Result<()> {
    let video_id = std::env::args()
        .nth(1)
        .expect("usage: test_stream <video_id>");

    let start = std::time::Instant::now();

    println!("Resolving direct URL...");
    let direct_url = extract::get_direct_url(&video_id).await?;
    println!("Resolved in {:.2}s", start.elapsed().as_secs_f64());

    println!("Opening streaming source...");
    let source = stream_source::HttpRangeSource::new(direct_url)?;

    println!("Decoding (streaming — should not wait for full download)...");
    let audio = decode::decode_from_source(Box::new(source), Some("m4a"))?;
    println!(
        "Decoded {:.1}s of audio, ready to play at {:.2}s since start",
        audio.duration_secs(),
        start.elapsed().as_secs_f64()
    );

    println!("Playing...");
    output::play_blocking(&audio)?;
    println!("Done.");

    Ok(())
}