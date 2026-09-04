use anyhow::Result;
use fung_preng_app_lib::{decode, extract};

#[tokio::main]
async fn main() -> Result<()> {
    let video_id = std::env::args()
        .nth(1)
        .expect("usage: test_decode <video_id>");

    println!("Extracting audio for video_id={video_id}...");
    let path = extract::extract_audio(&video_id).await?;
    println!("Extracted to {path:?}");

    println!("Decoding...");
    let audio = decode::decode_to_pcm(&path)?;

    println!("Sample rate: {} Hz", audio.sample_rate);
    println!("Channels: {}", audio.channels);
    println!("Total samples: {}", audio.samples.len());
    println!("Decoded duration: {:.1}s", audio.duration_secs());
    println!(
        "(compare this against the video's real length on YouTube — \
         it should match closely, within a second or so)"
    );

    // Clean up the temp file now that decoding is done with it — this is
    // the pattern the real pipeline will follow (decode fully, then discard).
    extract::cleanup(&path).await?;
    println!("Temp file cleaned up.");

    Ok(())
}