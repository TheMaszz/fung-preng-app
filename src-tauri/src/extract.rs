// src-tauri/src/extract.rs
//
// Wraps yt-dlp as a subprocess to pull an audio-only stream for a given
// YouTube video ID, writing it to a temp file. This is deliberately the
// only piece of the pipeline that touches the network / YouTube directly —
// keeping it isolated means everything downstream (decode, analyze, mix)
// never has to know or care where the audio came from.

use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;
use uuid::Uuid;

/// Pulls the best available audio-only stream for a YouTube video ID
/// and writes it to a temp file. Returns the path to that file.
///
/// Requires `yt-dlp` to be installed and on PATH.
/// (macOS: `brew install yt-dlp` | Windows: `winget install yt-dlp` | Linux: your package manager)
pub async fn extract_audio(video_id: &str) -> Result<PathBuf> {
    let url = format!("https://www.youtube.com/watch?v={video_id}");

    // Unique filename per call so concurrent extractions (current + next
    // track prefetch) never collide or overwrite each other.
    let temp_dir = std::env::temp_dir();
    let file_stem = format!("automix_{}", Uuid::new_v4());
    // yt-dlp picks the actual extension based on the source format (we let
    // it decide rather than forcing a re-encode, which would cost CPU and
    // a little quality for no benefit here).
    let output_template = temp_dir.join(format!("{file_stem}.%(ext)s"));

    let output = Command::new("yt-dlp")
        .arg("-f")
        // Prefer m4a (AAC) — Symphonia can decode this natively.
        // Falls back to bestaudio if no m4a stream exists for some video.
        .arg("bestaudio[ext=m4a]/bestaudio")
        .arg("-o")
        .arg(&output_template)
        .arg("--no-playlist")
        .arg("--no-warnings")
        .arg(&url)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output() // .output() (not .status()) so we can read stderr below
        .await
        .context("failed to spawn yt-dlp — is it installed and on PATH?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "yt-dlp exited with status {} for video_id={video_id}\nstderr:\n{stderr}",
            output.status
        ));
    }

    // yt-dlp substituted %(ext)s itself, so we need to find the actual
    // file it wrote. Since our stem is a fresh UUID, there's exactly one
    // match in the temp dir.
    let mut entries = tokio::fs::read_dir(&temp_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s == file_stem)
            .unwrap_or(false)
        {
            return Ok(path);
        }
    }

    Err(anyhow!(
        "yt-dlp reported success but no output file was found for video_id={video_id}"
    ))
}

/// Deletes a temp audio file once playback/analysis is done with it.
/// Call this after decode + (if needed) analysis have both finished reading
/// the file — this is what keeps disk usage bounded to ~current + next track.
pub async fn cleanup(path: &PathBuf) -> Result<()> {
    tokio::fs::remove_file(path)
        .await
        .with_context(|| format!("failed to remove temp file {path:?}"))
}