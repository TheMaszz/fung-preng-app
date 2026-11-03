// src-tauri/src/extract.rs
//
// Wraps yt-dlp to resolve / download audio for a YouTube video ID.
// This is the only piece of the pipeline that touches YouTube directly.

use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::Semaphore;
use uuid::Uuid;

/// At most this many yt-dlp processes run at once. Bursts of parallel
/// requests (related-track lookups + prep downloads + fallbacks) are what
/// make YouTube answer "The page needs to be reloaded" / 403.
const MAX_CONCURRENT_YTDLP: usize = 2;
static YTDLP_LIMIT: OnceLock<Semaphore> = OnceLock::new();
static YTDLP_EXECUTABLE: OnceLock<PathBuf> = OnceLock::new();

pub fn configure_ytdlp_executable(path: PathBuf) {
    let _ = YTDLP_EXECUTABLE.set(path);
}

pub fn ytdlp_executable() -> PathBuf {
    YTDLP_EXECUTABLE
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("yt-dlp"))
}

pub fn ytdlp_async_command() -> Command {
    let mut command = Command::new(ytdlp_executable());
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    command
}

pub fn ytdlp_sync_command() -> std::process::Command {
    let mut command = std::process::Command::new(ytdlp_executable());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command
}

fn ytdlp_limit() -> &'static Semaphore {
    YTDLP_LIMIT.get_or_init(|| Semaphore::new(MAX_CONCURRENT_YTDLP))
}

/// Recently downloaded audio, so a track downloaded as "Track B" of one mix
/// is not downloaded AGAIN as "Track A" of the next mix. Halves the number
/// of YouTube requests and makes prep faster.
// Keep a small byte-bounded cache. A track's compressed download is
// useful for the next transition, but retaining several long tracks can
// otherwise consume hundreds of MB on lower-memory machines.
const AUDIO_CACHE_MAX: usize = 2;
const AUDIO_CACHE_MAX_BYTES: usize = 48 * 1024 * 1024;
const AUDIO_CACHE_MAX_ENTRY_BYTES: usize = 32 * 1024 * 1024;
static AUDIO_CACHE: OnceLock<Mutex<Vec<(String, Arc<Vec<u8>>)>>> = OnceLock::new();
fn audio_cache() -> &'static Mutex<Vec<(String, Arc<Vec<u8>>)>> {
    AUDIO_CACHE.get_or_init(|| Mutex::new(Vec::new()))
}
fn cache_get(video_id: &str) -> Option<Arc<Vec<u8>>> {
    let guard = audio_cache().lock().unwrap_or_else(|p| p.into_inner());
    guard
        .iter()
        .find(|(id, _)| id == video_id)
        .map(|(_, bytes)| Arc::clone(bytes))
}
fn cache_put(video_id: &str, bytes: Vec<u8>) {
    if bytes.len() > AUDIO_CACHE_MAX_ENTRY_BYTES {
        return;
    }
    let mut guard = audio_cache().lock().unwrap_or_else(|p| p.into_inner());
    guard.retain(|(id, _)| id != video_id);
    guard.insert(0, (video_id.to_string(), Arc::new(bytes)));
    guard.truncate(AUDIO_CACHE_MAX);
    let mut total_bytes = 0usize;
    guard.retain(|(_, data)| {
        if total_bytes.saturating_add(data.len()) > AUDIO_CACHE_MAX_BYTES {
            return false;
        }
        total_bytes += data.len();
        true
    });
}

/// A resolved audio stream: the direct URL PLUS the exact HTTP headers
/// yt-dlp would send with it. YouTube URLs are tied to the client yt-dlp
/// used to get them (your log shows c=VISIONOS), so requesting them with
/// different headers (e.g. a hard-coded Chrome User-Agent) can get a 403.
pub struct ResolvedAudio {
    pub url: String,
    pub headers: HashMap<String, String>,
}

/// Resolves URL + headers using `yt-dlp -j` (JSON), without downloading.
pub async fn resolve_audio(video_id: &str) -> Result<ResolvedAudio> {
    let _permit = ytdlp_limit().acquire().await?;
    let video_url = format!("https://www.youtube.com/watch?v={video_id}");

    let output = ytdlp_async_command()
        .args([
            "-j",
            "-f",
            "ba[ext=m4a]/ba[acodec^=mp4a]/ba",
            "--no-playlist",
            "--no-warnings",
            "--force-ipv4",
            // NOTE: no --user-agent override on purpose. Let yt-dlp use the
            // UA that matches the client it resolved the URL with.
            &video_url,
        ])
        .output()
        .await
        .context("failed to spawn yt-dlp - is it installed and on PATH?")?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("yt-dlp failed: {err}"));
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("yt-dlp returned invalid JSON")?;

    let url = json["url"]
        .as_str()
        .filter(|u| !u.is_empty())
        .ok_or_else(|| anyhow!("yt-dlp JSON had no url"))?
        .to_string();

    let mut headers = HashMap::new();
    if let Some(map) = json["http_headers"].as_object() {
        for (k, v) in map {
            if let Some(v) = v.as_str() {
                headers.insert(k.clone(), v.to_string());
            }
        }
    }

    Ok(ResolvedAudio { url, headers })
}

async fn get_direct_url_once(video_id: &str, extra: &[String]) -> Result<String> {
    let _permit = ytdlp_limit().acquire().await?;
    let video_url = format!("https://www.youtube.com/watch?v={video_id}");
    let output = ytdlp_async_command()
        .args([
            "-g",
            "-f",
            "ba[ext=m4a]/ba[acodec^=mp4a]/ba",
            "--no-playlist",
            "--no-warnings",
            "--force-ipv4",
        ])
        .args(extra)
        .arg(&video_url)
        .output()
        .await
        .context("failed to spawn yt-dlp - is it installed and on PATH?")?;
    if !output.status.success() {
        return Err(anyhow!(
            "yt-dlp failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() {
        return Err(anyhow!("yt-dlp returned an empty URL"));
    }
    Ok(url)
}

/// Kept for existing callers. Returns just the URL. Retries with other
/// player clients and a short pause between attempts.
pub async fn get_direct_url(video_id: &str) -> Result<String> {
    let mut last_err = None;
    for (i, extra) in attempt_args().iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(Duration::from_millis(1500 * i as u64)).await;
        }
        match get_direct_url_once(video_id, extra).await {
            Ok(url) => return Ok(url),
            Err(e) => {
                let msg = format!("{e}");
                let permanent = is_permanent_error(&msg);
                last_err = Some(e);
                if permanent {
                    break;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("no attempts were made for {video_id}")))
}

fn is_permanent_error(msg: &str) -> bool {
    msg.contains("Video unavailable")
        || msg.contains("Private video")
        || msg.contains("has been removed")
}

pub async fn get_duration_secs(video_id: &str) -> Result<Option<f64>> {
    let _permit = ytdlp_limit().acquire().await?;
    let video_url = format!("https://www.youtube.com/watch?v={video_id}");
    let output = ytdlp_async_command()
        .args([
            "--no-warnings",
            "--no-playlist",
            "--skip-download",
            "--print",
            "%(duration)s",
            &video_url,
        ])
        .output()
        .await
        .context("failed to spawn yt-dlp - is it installed and on PATH?")?;

    if !output.status.success() {
        return Err(anyhow!(
            "yt-dlp failed to read track duration: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let output = String::from_utf8_lossy(&output.stdout);
    let duration = output.trim();
    if duration.is_empty() || duration == "NA" {
        return Ok(None);
    }

    let duration_secs = duration
        .parse::<f64>()
        .context("yt-dlp returned an invalid track duration")?;
    Ok((duration_secs.is_finite() && duration_secs > 0.0).then_some(duration_secs))
}

/// Downloads the full audio into memory. Retries with a freshly resolved
/// URL on failure, and finally falls back to yt-dlp's own downloader
/// (`extract_audio`), which handles every YouTube quirk itself.
///
/// Decode the result with:
///   decode_full_track(std::io::Cursor::new(bytes))
pub async fn download_audio_bytes(video_id: &str) -> Result<Vec<u8>> {
    if let Some(hit) = cache_get(video_id) {
        println!("[EXTRACT] {video_id} served from audio cache");
        return Ok(hit.as_ref().clone());
    }
    let path = extract_audio(video_id).await?;
    let bytes = tokio::fs::read(&path)
        .await
        .with_context(|| format!("failed to read {path:?}"))?;
    let _ = cleanup(&path).await;
    cache_put(video_id, bytes.clone());
    Ok(bytes)
}

/// Extra yt-dlp args to try, in order. First attempt = plain defaults.
/// YouTube returns 403 for some videos on some player clients, so we retry
/// with different clients. Client names change between yt-dlp releases -
/// keep this list easy to edit, and keep yt-dlp updated (`yt-dlp -U`).
fn attempt_args() -> Vec<Vec<String>> {
    let mut attempts: Vec<Vec<String>> = vec![
        vec![],
        vec![
            "--extractor-args".into(),
            "youtube:player_client=android_vr".into(),
        ],
        vec![
            "--extractor-args".into(),
            "youtube:player_client=tv,web_safari".into(),
        ],
    ];
    // Optional last resort: set AUTOMIX_COOKIES_BROWSER=firefox (or chrome,
    // edge, ...) to retry using cookies from that browser.
    if let Ok(browser) = std::env::var("AUTOMIX_COOKIES_BROWSER") {
        if !browser.trim().is_empty() {
            attempts.push(vec!["--cookies-from-browser".into(), browser]);
        }
    }
    attempts
}

async fn extract_audio_once(video_id: &str, extra: &[String]) -> Result<PathBuf> {
    let _permit = ytdlp_limit().acquire().await?;
    let url = format!("https://www.youtube.com/watch?v={video_id}");

    let temp_dir = std::env::temp_dir();
    let file_stem = format!("automix_{}", Uuid::new_v4());
    let output_template = temp_dir.join(format!("{file_stem}.%(ext)s"));

    let output = ytdlp_async_command()
        .arg("-f")
        .arg("bestaudio[ext=m4a]/bestaudio")
        .arg("-o")
        .arg(&output_template)
        .arg("--no-playlist")
        .arg("--no-warnings")
        .arg("--force-ipv4")
        .args(["--retries", "3", "--fragment-retries", "3"])
        .args(extra)
        .arg(&url)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .context("failed to spawn yt-dlp - is it installed and on PATH?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "yt-dlp exited with status {} for video_id={video_id}\nstderr:\n{stderr}",
            output.status
        ));
    }

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

/// Pulls the best available audio-only stream for a YouTube video ID and
/// writes it to a temp file. Retries with different YouTube player clients
/// (and optionally browser cookies) when a download fails, e.g. with a 403.
pub async fn extract_audio(video_id: &str) -> Result<PathBuf> {
    let mut last_err = None;

    for (i, extra) in attempt_args().iter().enumerate() {
        // Back off between attempts: YouTube's "page needs to be reloaded"
        // / 403 answers are often transient, and instant retries just make
        // it worse.
        if i > 0 {
            tokio::time::sleep(Duration::from_millis(1500 * i as u64)).await;
        }
        match extract_audio_once(video_id, extra).await {
            Ok(path) => {
                if i > 0 {
                    eprintln!(
                        "[EXTRACT] {video_id} succeeded on attempt {} (args: {:?})",
                        i + 1,
                        extra
                    );
                }
                return Ok(path);
            }
            Err(e) => {
                let msg = format!("{e}");
                eprintln!(
                    "[EXTRACT] {video_id} attempt {} failed: {}",
                    i + 1,
                    msg.lines().next().unwrap_or("")
                );
                // No point retrying videos that are gone or private.
                let permanent = is_permanent_error(&msg);
                last_err = Some(e);
                if permanent {
                    break;
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow!("no download attempts were made for {video_id}")))
}

/// Deletes a temp audio file once playback/analysis is done with it.
pub async fn cleanup(path: &PathBuf) -> Result<()> {
    tokio::fs::remove_file(path)
        .await
        .with_context(|| format!("failed to remove temp file {path:?}"))
}
