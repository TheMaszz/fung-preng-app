// src-tauri/src/commands.rs
use crate::cache::CacheManager;
use crate::extract;
use crate::player_state::{AppPlayerState, PlaybackProgress};
use crate::stream_player;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn play_audio(
    video_id: String,
    duration_secs: Option<f64>,
    direct_url: Option<String>,
    state: State<'_, AppPlayerState>,
    cache: State<'_, CacheManager>,
) -> Result<(), String> {
    let old_cancel = state
        .cancel_flag
        .lock()
        .map_err(|_| "player state lock poisoned".to_string())?
        .clone();
    old_cancel.store(true, Ordering::SeqCst);

    let new_cancel = Arc::new(AtomicBool::new(false));
    *state
        .cancel_flag
        .lock()
        .map_err(|_| "player state lock poisoned".to_string())? = Arc::clone(&new_cancel);
    state.is_paused.store(false, Ordering::SeqCst);
    if let Ok(mut progress) = state.progress.lock() {
        progress.position_secs = 0.0;
        progress.duration_secs = duration_secs.unwrap_or(0.0).max(0.0);
    }

    let cancel_flag = Arc::clone(&new_cancel);
    let is_paused = Arc::clone(&state.is_paused);
    let volume = Arc::clone(&state.volume);
    let progress = Arc::clone(&state.progress);

    let url_start = std::time::Instant::now();
    let resolved_url = match direct_url {
        Some(url) if !url.trim().is_empty() => {
            println!(
                "[TIMER] Used direct URL provided by frontend in {:?}",
                url_start.elapsed()
            );
            url
        }
        _ => {
            if let Some(cached) = cache.get_cached_url(&video_id) {
                println!(
                    "[TIMER] Fetched URL from RAM Cache in {:?}",
                    url_start.elapsed()
                );
                cached
            } else {
                let url = extract::get_direct_url(&video_id)
                    .await
                    .map_err(|e| e.to_string())?;
                cache.save_url_cache(video_id.clone(), url.clone());
                println!(
                    "[TIMER] Executed yt-dlp on click in {:?}",
                    url_start.elapsed()
                );
                url
            }
        }
    };

    std::thread::spawn(move || {
        let _stream_start = std::time::Instant::now();
        if let Err(e) = stream_player::play_streaming_with_cancel(
            resolved_url,
            Some("m4a"),
            cancel_flag,
            is_paused,
            volume,
            progress,
        ) {
            eprintln!("Playback error: {e}");
        }
        println!(
            "[TIMER] Total time from click to streaming completion: {:?}",
            _stream_start.elapsed()
        );
    });

    Ok(())
}

#[tauri::command]
pub async fn prefetch_track_urls(
    video_ids: Vec<String>,
    cache: State<'_, CacheManager>,
) -> Result<(), String> {
    let cache_manager = cache.inner().clone();

    tokio::spawn(async move {
        for video_id in video_ids.into_iter().take(3) {
            if cache_manager.get_cached_url(&video_id).is_some() {
                continue;
            }
            if let Ok(url) = extract::get_direct_url(&video_id).await {
                cache_manager.save_url_cache(video_id, url);
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn resolve_audio_url(
    video_id: String,
    cache: State<'_, CacheManager>,
) -> Result<String, String> {
    if let Some(cached) = cache.get_cached_url(&video_id) {
        return Ok(cached);
    }
    let url = extract::get_direct_url(&video_id)
        .await
        .map_err(|error| error.to_string())?;
    cache.save_url_cache(video_id.clone(), url.clone());
    Ok(url)
}

#[tauri::command]
pub fn get_playback_progress(state: State<'_, AppPlayerState>) -> Result<PlaybackProgress, String> {
    state
        .progress
        .lock()
        .map(|progress| *progress)
        .map_err(|_| "player state lock poisoned".to_string())
}

#[tauri::command]
pub fn stop_audio(state: State<'_, AppPlayerState>) -> Result<(), String> {
    let cancel_flag = state
        .cancel_flag
        .lock()
        .map_err(|_| "player state lock poisoned".to_string())?
        .clone();
    cancel_flag.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub fn set_volume(volume: f32, state: State<'_, AppPlayerState>) -> Result<(), String> {
    if let Ok(mut vol) = state.volume.lock() {
        *vol = volume.clamp(0.0, 1.0);
    }
    Ok(())
}

#[tauri::command]
pub fn pause_audio(state: State<'_, AppPlayerState>) -> Result<(), String> {
    state.is_paused.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub fn resume_audio(state: State<'_, AppPlayerState>) -> Result<(), String> {
    state.is_paused.store(false, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}