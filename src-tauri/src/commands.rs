// src-tauri/src/commands.rs
use crate::analyze::Analyzer;
use crate::cache::{CacheManager, CacheStats};
use crate::database::{AppDatabase, ListenHistoryEntry, Playlist, PlaylistTrack};
use crate::extract;
use crate::mixer::{MixedTrack, Mixer};
use crate::player_state::PlaybackProgress;
use crate::stream_player;
use crate::stream_source::HttpRangeSource;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

// Crossfade mixes that were built ahead of time in the background, waiting
// to be swapped in the instant we want to transition tracks. Keyed by the
// exact (video_id_a, video_id_b) pair they were built for - NOT a single
// overwritable slot. Multiple prep jobs can legitimately be in flight at
// once (e.g. dev-mode double effect invocations, or prep for track N+1
// still finishing while prep for N+2 already started), and a single slot
// meant whichever job finished last would silently clobber an unrelated
// pair's mix, forcing an unnecessary slow fallback right when it was
// needed most.
pub type PreparedMixCache = Arc<Mutex<HashMap<(String, String), MixedTrack>>>;

// โครงสร้าง AppState สำหรับจัดการ State สัญญาณเสียง
pub struct AppState {
    pub current_cancel_flag: Mutex<Option<Arc<AtomicBool>>>,
    pub volume: Arc<AtomicU32>,        // ใช้ AtomicU32 เก็บ float bits
    pub progress_secs: Arc<AtomicU32>, // บันทึกเวลาปัจจุบัน (วินาที)
    pub duration_secs: Arc<AtomicU32>, // ความยาวเพลงทั้งหมด (วินาที)
    pub is_paused: Arc<AtomicBool>,
    pub eq_enabled: Arc<AtomicBool>,
    pub eq_gains: Arc<Mutex<[f32; 7]>>,
    pub prepared_mixes: PreparedMixCache,
    // Caps how many decode+analyze+mix pipelines can run at once. Each one
    // is CPU-heavy; letting several run concurrently (e.g. after a network
    // hiccup causes a burst of back-to-back transitions) makes every one of
    // them slower, which causes more transitions to miss their prep
    // window, which triggers more concurrent builds - a snowball that
    // shows up as decode times climbing from ~7s to 30+s over a session.
    // Serializing builds keeps each one fast even under a burst.
    pub mix_build_semaphore: Arc<tokio::sync::Semaphore>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            current_cancel_flag: Mutex::new(None),
            volume: Arc::new(AtomicU32::new(1.0f32.to_bits())), // Default 100% (1.0)
            progress_secs: Arc::new(AtomicU32::new(0)),
            duration_secs: Arc::new(AtomicU32::new(0)),
            is_paused: Arc::new(AtomicBool::new(false)),
            eq_enabled: Arc::new(AtomicBool::new(true)),
            eq_gains: Arc::new(Mutex::new([0.0; 7])),
            prepared_mixes: Arc::new(Mutex::new(HashMap::new())),
            mix_build_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct TrackInfo {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub duration_secs: Option<f64>,
    pub thumbnail_url: Option<String>,
}

#[tauri::command]
pub fn record_listen_history(
    id: String,
    title: String,
    channel: String,
    database: State<'_, AppDatabase>,
) -> Result<(), String> {
    database.record_listen(&id, &title, &channel)
}

#[tauri::command]
pub fn import_legacy_listen_history(
    entries: Vec<ListenHistoryEntry>,
    database: State<'_, AppDatabase>,
) -> Result<(), String> {
    database.import_legacy_history(entries)
}

#[tauri::command]
pub fn get_listen_history(
    database: State<'_, AppDatabase>,
) -> Result<Vec<ListenHistoryEntry>, String> {
    database.listen_history()
}

#[tauri::command]
pub fn clear_listen_history(database: State<'_, AppDatabase>) -> Result<(), String> {
    database.clear_listen_history()
}

#[tauri::command]
pub fn get_playlists(database: State<'_, AppDatabase>) -> Result<Vec<Playlist>, String> {
    database.playlists()
}

#[tauri::command]
pub fn create_playlist(name: String, database: State<'_, AppDatabase>) -> Result<Playlist, String> {
    database.create_playlist(&name)
}

#[tauri::command]
pub fn delete_playlist(playlist_id: i64, database: State<'_, AppDatabase>) -> Result<(), String> {
    database.delete_playlist(playlist_id)
}

#[tauri::command]
pub fn add_playlist_track(
    playlist_id: i64,
    id: String,
    title: String,
    channel: String,
    duration_secs: Option<f64>,
    thumbnail_url: Option<String>,
    database: State<'_, AppDatabase>,
) -> Result<bool, String> {
    database.add_playlist_track(
        playlist_id,
        PlaylistTrack {
            id,
            title,
            channel,
            duration_secs,
            thumbnail_url,
            position: 0,
        },
    )
}

#[tauri::command]
pub fn add_track_to_playlist(
    playlist_id: i64,
    track: TrackInfo,
    database: State<'_, AppDatabase>,
) -> Result<bool, String> {
    database.add_playlist_track(
        playlist_id,
        PlaylistTrack {
            id: track.id,
            title: track.title,
            channel: track.channel,
            duration_secs: track.duration_secs,
            thumbnail_url: track.thumbnail_url,
            position: 0,
        },
    )
}

#[tauri::command]
pub fn get_playlist_tracks(
    playlist_id: i64,
    database: State<'_, AppDatabase>,
) -> Result<Vec<PlaylistTrack>, String> {
    database.playlist_tracks(playlist_id)
}

#[tauri::command]
pub fn reorder_playlist_tracks(
    playlist_id: i64,
    track_ids: Vec<String>,
    database: State<'_, AppDatabase>,
) -> Result<(), String> {
    database.reorder_playlist_tracks(playlist_id, &track_ids)
}

#[tauri::command]
pub fn remove_track_from_playlist(
    playlist_id: i64,
    track_id: String,
    database: State<'_, AppDatabase>,
) -> Result<(), String> {
    database.remove_playlist_track(playlist_id, &track_id)
}

// Command สำหรับสั่งเล่นเพลง (แก้ปัญหา Mixing เพลงซ้อน)
#[tauri::command]
pub async fn play_audio(
    video_id: String,
    duration_secs: Option<u64>,
    direct_url: Option<String>,
    state: State<'_, AppState>,
    cache: State<'_, CacheManager>,
    app_handle: AppHandle,
) -> Result<(), String> {
    // 1. แก้ไขปัญหา Mixing: สั่งหยุด (Cancel) เพลงเก่าทันที
    {
        let mut flag_guard = state
            .current_cancel_flag
            .lock()
            .map_err(|_| "Failed to lock cancel flag")?;

        if let Some(old_flag) = flag_guard.take() {
            old_flag.store(true, Ordering::SeqCst);
        }

        let new_flag = Arc::new(AtomicBool::new(false));
        *flag_guard = Some(Arc::clone(&new_flag));
    }

    let cancel_flag = {
        let flag_guard = state.current_cancel_flag.lock().unwrap();
        Arc::clone(flag_guard.as_ref().unwrap())
    };

    // A brand new, non-crossfaded track start invalidates any in-flight or
    // cached crossfade prep tied to whatever was playing before - none of
    // it is relevant to this new track.
    {
        let mut prepared_guard = state.prepared_mixes.lock().unwrap();
        prepared_guard.clear();
    }

    // Reset Progress เวลาสำหรับเพลงใหม่
    state.progress_secs.store(0, Ordering::Relaxed);
    state
        .duration_secs
        .store(duration_secs.unwrap_or(0) as u32, Ordering::Relaxed);

    // 2. Resolve URL สตรีมมิ่ง (ใช้ Direct URL ถ้ามี หรือหาผ่าน Cache/yt-dlp)
    let stream_url = if let Some(url) = direct_url {
        url
    } else if let Some(cached_url) = cache.get_cached_url(&video_id) {
        cached_url
    } else {
        let url = extract::get_direct_url(&video_id)
            .await
            .map_err(|e| format!("Failed to extract direct URL: {e}"))?;
        cache.save_url_cache(video_id.clone(), url.clone());
        url
    };

    let volume = Arc::clone(&state.volume);
    let progress_secs = Arc::clone(&state.progress_secs);
    let is_paused = Arc::clone(&state.is_paused);
    let eq_enabled = Arc::clone(&state.eq_enabled);
    let eq_gains = Arc::clone(&state.eq_gains);

    std::thread::spawn(move || {
        let reader = match HttpRangeSource::new(stream_url) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Failed to open HttpRangeSource: {}", e);
                return;
            }
        };

        if let Err(e) = stream_player::play_stream(
            reader,
            cancel_flag,
            volume,
            progress_secs,
            is_paused,
            eq_enabled,
            eq_gains,
            app_handle,
        ) {
            eprintln!("Playback error: {}", e);
        }
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

// อ่าน Progress ตำแหน่งเวลาส่งให้ Frontend
#[tauri::command]
pub fn get_playback_progress(state: State<'_, AppState>) -> Result<PlaybackProgress, String> {
    let position_secs = state.progress_secs.load(Ordering::Relaxed) as f64;
    let duration_secs = state.duration_secs.load(Ordering::Relaxed) as f64;

    Ok(PlaybackProgress {
        position_secs,
        duration_secs,
    })
}

#[tauri::command]
pub fn stop_audio(state: State<'_, AppState>) -> Result<(), String> {
    let mut flag_guard = state
        .current_cancel_flag
        .lock()
        .map_err(|_| "Failed to lock cancel flag")?;

    if let Some(flag) = flag_guard.take() {
        flag.store(true, Ordering::SeqCst);
    }
    Ok(())
}

// ปรับ Volume แบบไร้ดีเลย์ ( Real-time Atomic Store )
#[tauri::command]
pub fn set_volume(volume: f32, state: State<'_, AppState>) {
    let clamped = volume.clamp(0.0, 1.0);
    state.volume.store(clamped.to_bits(), Ordering::Relaxed);
}

#[tauri::command]
pub fn set_eq(enabled: bool, gains: Vec<f32>, state: State<'_, AppState>) -> Result<(), String> {
    let mut normalized = [0.0f32; 7];
    if gains.len() != 7 {
        return Err("Equalizer requires exactly 7 gain values".to_string());
    }

    for (slot, value) in normalized.iter_mut().zip(gains.iter()) {
        *slot = value.clamp(-12.0, 12.0);
    }

    state.eq_enabled.store(enabled, Ordering::Relaxed);
    *state
        .eq_gains
        .lock()
        .map_err(|_| "Failed to lock eq gains")? = normalized;
    Ok(())
}

#[tauri::command]
pub fn pause_audio(state: State<'_, AppState>) -> Result<(), String> {
    state.is_paused.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub fn resume_audio(state: State<'_, AppState>) -> Result<(), String> {
    state.is_paused.store(false, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn get_related_tracks(video_id: String) -> Result<Vec<TrackInfo>, String> {
    let mix_url = format!(
        "https://www.youtube.com/watch?v={}&list=RD{}",
        video_id, video_id
    );

    let output = tokio::process::Command::new("yt-dlp")
        .args([
            "--flat-playlist",
            "-J",
            "--no-warnings",
            "--playlist-end",
            "15",
            &mix_url,
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to fetch related tracks: {e}"))?;

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("Failed to parse JSON: {e}"))?;

    let mut tracks = Vec::new();
    if let Some(entries) = json.get("entries").and_then(|e| e.as_array()) {
        for entry in entries {
            let id = entry
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            if id.is_empty() || id == video_id {
                continue;
            }

            let title = entry
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();
            let channel = entry
                .get("uploader")
                .or_else(|| entry.get("channel"))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();
            let duration_secs = entry.get("duration").and_then(|v| v.as_f64());
            let thumbnail_url = entry
                .get("thumbnails")
                .and_then(|t| t.as_array())
                .and_then(|arr| arr.last())
                .and_then(|t| t.get("url"))
                .and_then(|u| u.as_str())
                .map(|s| s.to_string());

            tracks.push(TrackInfo {
                id,
                title,
                channel,
                duration_secs,
                thumbnail_url,
            });
        }
    }

    println!(
        "[RELATED] Found {} related tracks for {}",
        tracks.len(),
        video_id
    );

    Ok(tracks)
}

// Builds the crossfade mix for (video_id_a -> video_id_b) in the background
// and stashes it in AppState.prepared_mix, WITHOUT touching playback at all.
// This is the piece that was missing before: it lets us pay the expensive
// download + decode + loudness-analysis + mix cost *while track A is still
// playing*, so that by the time we actually want to transition, the mixed
// buffer is just sitting there ready to go instead of being built from
// scratch during a dead silence.
// Emitted once a background mix finishes, telling the frontend the exact
// position (in seconds, within Track A's own playback) at which it should
// hand off from normal playback to the mixed buffer. Without this, the
// frontend can only guess at a fixed "N seconds before the end" - which
// doesn't necessarily line up with where the mix's crossfade actually
// starts, causing either a skipped/repeated few seconds at the swap.
#[derive(Clone, serde::Serialize)]
struct MixReadyPayload {
    video_id_a: String,
    video_id_b: String,
    transition_at_secs: f64,
}

#[derive(Clone, serde::Serialize)]
struct MixPrepFailedPayload {
    video_id_a: String,
    video_id_b: String,
}

#[tauri::command]
pub async fn prepare_mixed_session(
    video_id_a: String,
    video_id_b: String,
    state: State<'_, AppState>,
    cache: State<'_, CacheManager>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let pair_key = (video_id_a.clone(), video_id_b.clone());
    let pair_label = format!("{}->{}", video_id_a, video_id_b);

    // Don't redo work if this exact pair is already prepared.
    {
        let existing = state.prepared_mixes.lock().unwrap();
        if existing.contains_key(&pair_key) {
            println!("[MIX PREP] [{pair_label}] Already prepared, skipping");
            return Ok(());
        }
    }

    // Resolve both stream URLs at the same time instead of one after another.
    let (url_a, url_b) = tokio::try_join!(
        resolve_audio_url(video_id_a.clone(), cache.clone()),
        resolve_audio_url(video_id_b.clone(), cache.clone())
    )?;

    let prepared_mixes = Arc::clone(&state.prepared_mixes);
    let mix_build_semaphore = Arc::clone(&state.mix_build_semaphore);

    tokio::task::spawn_blocking(move || {
        // Wait for a build slot before doing any decode/analyze/mix work,
        // so a burst of transitions queues up instead of running several
        // CPU-heavy builds at once and slowing each other down. We're
        // still inside the tokio runtime here (just on its blocking pool),
        // so Handle::block_on works fine for the async Semaphore.
        let _permit = tokio::runtime::Handle::current()
            .block_on(mix_build_semaphore.acquire_owned())
            .expect("mix build semaphore closed");

        let prep_start = std::time::Instant::now();
        println!("[MIX PREP] [{pair_label}] Starting");

        // Single place to bail out of this build: logs, tells the frontend
        // this pair failed (so it can clear mixPrepKeyRef and retry)
        // instead of silently leaving the frontend waiting on a pair that
        // will never arrive, and returns.
        let fail = |msg: String| {
            eprintln!("[MIX PREP] [{pair_label}] {}", msg);
            let _ = app_handle.emit(
                "mix-prep-failed",
                MixPrepFailedPayload {
                    video_id_a: video_id_a.clone(),
                    video_id_b: video_id_b.clone(),
                },
            );
        };

        // Decode Track B on its own OS thread so it happens concurrently
        // with decoding Track A below - this is typically the single
        // biggest chunk of the pipeline, so doing it in parallel roughly
        // halves that portion of the wait.
        let decode_b_handle = std::thread::spawn(move || -> Result<_, String> {
            let reader_b = HttpRangeSource::new(url_b).map_err(|e| e.to_string())?;
            stream_player::decode_full_track(reader_b).map_err(|e| e.to_string())
        });

        let reader_a = match HttpRangeSource::new(url_a) {
            Ok(r) => r,
            Err(e) => return fail(format!("Failed opening Track A: {}", e)),
        };
        let (samples_a, rate_a, ch_a) = match stream_player::decode_full_track(reader_a) {
            Ok(data) => data,
            Err(e) => return fail(format!("Failed decoding Track A: {}", e)),
        };

        let (samples_b, rate_b, ch_b) = match decode_b_handle.join() {
            Ok(Ok(data)) => data,
            Ok(Err(e)) => return fail(format!("Failed decoding Track B: {}", e)),
            Err(_) => return fail("Track B decode thread panicked".to_string()),
        };

        println!(
            "[MIX PREP] [{pair_label}] Decode finished in {:.2}s",
            prep_start.elapsed().as_secs_f32()
        );

        let analyzer_a = Analyzer::new(rate_a, ch_a);
        let analysis_a = match analyzer_a.analyze(&samples_a) {
            Ok(res) => res,
            Err(e) => return fail(format!("Analysis failed for Track A: {}", e)),
        };

        // Analyze B at its own native rate/channels BEFORE any conversion -
        // intro/outro points are measured in seconds, so this must run on
        // the original decode, matching the Analyzer it's configured with.
        let analyzer_b = Analyzer::new(rate_b, ch_b);
        let analysis_b = match analyzer_b.analyze(&samples_b) {
            Ok(res) => res,
            Err(e) => return fail(format!("Analysis failed for Track B: {}", e)),
        };

        // Track B may have decoded at a different native rate/channel count
        // than Track A (e.g. 48kHz vs 44.1kHz source streams). Mixer::mix
        // assumes both buffers already share one sample_rate/channels, so
        // align B onto A's format here - otherwise B gets crossfaded and
        // played back at the wrong speed/pitch, and its declared
        // track_b_total_secs stops matching how long it actually plays for,
        // which throws off the *next* transition's timing.
        let samples_b = if rate_b != rate_a || ch_b != ch_a {
            eprintln!(
                "[MIX PREP] [{pair_label}] Track B is {}Hz/{}ch, Track A is {}Hz/{}ch - resampling B to match",
                rate_b, ch_b, rate_a, ch_a
            );
            match crate::resample::align_track(&samples_b, rate_b, ch_b, rate_a, ch_a) {
                Ok(s) => s,
                Err(e) => return fail(format!("Failed aligning Track B: {}", e)),
            }
        } else {
            samples_b
        };

        let mixed: MixedTrack = match Mixer::mix(
            &samples_a,
            &analysis_a,
            &samples_b,
            &analysis_b,
            rate_a,
            ch_a,
            -14.0, // LUFS target
        ) {
            Ok(m) => m,
            Err(e) => return fail(format!("Mixer error: {}", e)),
        };

        let transition_at_secs = mixed.transition_at_secs;

        {
            let mut guard = prepared_mixes.lock().unwrap();
            guard.insert(pair_key, mixed);
        }

        println!(
            "[MIX PREP] [{pair_label}] Ready in {:.2}s total (transition at {:.2}s into Track A)",
            prep_start.elapsed().as_secs_f32(),
            transition_at_secs
        );

        let _ = app_handle.emit(
            "mix-ready",
            MixReadyPayload {
                video_id_a,
                video_id_b,
                transition_at_secs,
            },
        );
    });

    Ok(())
}

#[tauri::command]
pub async fn play_mixed_session(
    video_id_a: String,
    video_id_b: String,
    state: State<'_, AppState>,
    cache: State<'_, CacheManager>,
    app_handle: AppHandle,
) -> Result<(), String> {
    // Grab a handle to the currently-playing stream's cancel flag WITHOUT
    // signaling it yet. Opening a brand-new cpal output stream has real
    // setup latency (device open, buffer negotiation, first callback) - if
    // we kill the old stream before the new one is actually producing
    // sound, there's an audible silent gap between them. Since the
    // crossfade buffer itself starts at full Track-A volume (fading B in
    // from there), a brief overlap where both streams are technically
    // alive is inaudible as doubling - but a hard cut-then-gap is very
    // audible, which is what was happening before.
    let old_cancel_flag = {
        let flag_guard = state.current_cancel_flag.lock().unwrap();
        flag_guard.clone()
    };

    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut flag_guard = state.current_cancel_flag.lock().unwrap();
        *flag_guard = Some(Arc::clone(&cancel_flag));
    }

    state.is_paused.store(false, Ordering::SeqCst);
    state.progress_secs.store(0, Ordering::Relaxed);

    let volume = Arc::clone(&state.volume);
    let progress_secs = Arc::clone(&state.progress_secs);
    let is_paused = Arc::clone(&state.is_paused);
    let eq_enabled = Arc::clone(&state.eq_enabled);
    let eq_gains = Arc::clone(&state.eq_gains);
    let duration_secs_state = Arc::clone(&state.duration_secs);

    // Reuse a mix that prepare_mixed_session already built in the
    // background, if it matches this exact pair. This is what makes the
    // transition feel instant instead of waiting on a fresh
    // download + decode + analyze + mix cycle. Also clear out any other
    // cached pairs at this point - once we're actually transitioning past
    // Track A, prep results for any other (Track A -> something-else)
    // pairing are no longer relevant.
    let preloaded = {
        let mut guard = state.prepared_mixes.lock().unwrap();
        let pair_key = (video_id_a.clone(), video_id_b.clone());
        let found = guard.remove(&pair_key);
        guard.retain(|(a, _), _| a != &video_id_a);
        found
    };

    if let Some(mixed) = preloaded {
        println!("[MIX] Using pre-built crossfade for {video_id_a} -> {video_id_b}");

        // Report the *true* total length of Track B, not just this
        // buffer's own (intro-skipped) sample count - otherwise "remaining
        // time" calculations on the frontend end up in a different time
        // frame than transition_at_secs, which is always measured from
        // Track B's true position 0.
        duration_secs_state.store(mixed.track_b_total_secs as u32, Ordering::Relaxed);
        progress_secs.store(mixed.starts_at_secs_in_b as u32, Ordering::Relaxed);

        let initial_offset_secs = mixed.starts_at_secs_in_b;

        tokio::task::spawn_blocking(move || {
            if let Err(e) = stream_player::play_pcm_buffer(
                mixed.samples,
                mixed.sample_rate,
                mixed.channels,
                initial_offset_secs,
                cancel_flag,
                volume,
                progress_secs,
                is_paused,
                eq_enabled,
                eq_gains,
                app_handle,
            ) {
                eprintln!("Playback error: {}", e);
            }
        });

        // Let the new stream actually get up and running - open the
        // device, start pushing its first samples - before we stop the
        // old one. This overlap (not a hard cut) is what makes the handoff
        // sound seamless.
        if let Some(old_flag) = old_cancel_flag {
            tokio::time::sleep(std::time::Duration::from_millis(180)).await;
            old_flag.store(true, Ordering::SeqCst);
        }

        return Ok(());
    }

    // Fallback: nothing was prepared ahead of time (e.g. the transition
    // happened sooner than prep could finish, or prep failed/got
    // outrun). This still works, it just won't be instant - there will be
    // an audible gap while we decode/analyze/mix both tracks from scratch.
    println!("[MIX] [{video_id_a}->{video_id_b}] No pre-built crossfade available, building now (will have a gap)");
    let (url_a, url_b) = tokio::try_join!(
        resolve_audio_url(video_id_a.clone(), cache.clone()),
        resolve_audio_url(video_id_b.clone(), cache.clone())
    )?;

    let mix_build_semaphore = Arc::clone(&state.mix_build_semaphore);

    tokio::task::spawn_blocking(move || {
        // Same build-slot wait as prepare_mixed_session - this path is the
        // one most likely to be competing with other builds (it only runs
        // when we're already behind), so it matters here just as much.
        let _permit = tokio::runtime::Handle::current()
            .block_on(mix_build_semaphore.acquire_owned())
            .expect("mix build semaphore closed");

        let reader_a = match HttpRangeSource::new(url_a) {
            Ok(r) => r,
            Err(e) => return eprintln!("Failed opening Track A: {}", e),
        };
        let reader_b = match HttpRangeSource::new(url_b) {
            Ok(r) => r,
            Err(e) => return eprintln!("Failed opening Track B: {}", e),
        };

        // Decode Track A & Track B fully
        let (samples_a, rate_a, ch_a) = match stream_player::decode_full_track(reader_a) {
            Ok(data) => data,
            Err(e) => return eprintln!("Failed decoding Track A: {}", e),
        };
        let (samples_b, rate_b, ch_b) = match stream_player::decode_full_track(reader_b) {
            Ok(data) => data,
            Err(e) => return eprintln!("Failed decoding Track B: {}", e),
        };

        // Run analysis on both tracks using your Analyzer struct
        let analyzer_a = Analyzer::new(rate_a, ch_a);
        let analysis_a = match analyzer_a.analyze(&samples_a) {
            Ok(res) => res,
            Err(e) => return eprintln!("Analysis failed for Track A: {}", e),
        };

        let analyzer_b = Analyzer::new(rate_b, ch_b);
        let analysis_b = match analyzer_b.analyze(&samples_b) {
            Ok(res) => res,
            Err(e) => return eprintln!("Analysis failed for Track B: {}", e),
        };

        // Align Track B onto Track A's rate/channels before mixing - see
        // the matching comment in prepare_mixed_session for why.
        let samples_b = if rate_b != rate_a || ch_b != ch_a {
            eprintln!(
                "[{video_id_a}->{video_id_b}] Track B is {}Hz/{}ch, Track A is {}Hz/{}ch - resampling B to match",
                rate_b, ch_b, rate_a, ch_a
            );
            match crate::resample::align_track(&samples_b, rate_b, ch_b, rate_a, ch_a) {
                Ok(s) => s,
                Err(e) => return eprintln!("Failed aligning Track B: {}", e),
            }
        } else {
            samples_b
        };

        // Mix both analyzed tracks
        let mixed: MixedTrack = match Mixer::mix(
            &samples_a,
            &analysis_a,
            &samples_b,
            &analysis_b,
            rate_a,
            ch_a,
            -14.0, // LUFS target
        ) {
            Ok(m) => m,
            Err(e) => return eprintln!("Mixer error: {}", e),
        };

        // The old Track A stream keeps playing live, in real time, the
        // whole time this fallback mix was building (which can take many
        // seconds - see the timing note above). So by the time `mixed` is
        // ready, Track A has very likely already played past
        // mixed.transition_at_secs, the fixed point this buffer's crossfade
        // was built to start at. Swapping in the buffer unmodified would
        // make playback jump BACKWARD to that stale point before easing
        // into Track B - audible as the current song "cutting off" and
        // restarting a few seconds earlier. `progress_secs` is still being
        // updated by the old stream's audio callback right up until we
        // cancel it below, so read it now (before we overwrite it) to see
        // where Track A actually is, and skip the buffer forward to match.
        let old_track_a_now_secs = progress_secs.load(Ordering::Relaxed) as f64;
        let stale_by_secs = (old_track_a_now_secs - mixed.transition_at_secs).max(0.0);
        let bytes_per_sec = mixed.sample_rate as f64 * mixed.channels as f64;
        let skip_samples = ((stale_by_secs * bytes_per_sec) as usize).min(mixed.samples.len());

        if skip_samples > 0 {
            eprintln!(
                "[MIX] [{video_id_a}->{video_id_b}] Track A ran {:.2}s past the crossfade point while this fallback mix was building ({:.2}s) - skipping ahead to avoid a rewind",
                stale_by_secs, stale_by_secs
            );
        }

        let samples_to_play = if skip_samples > 0 {
            mixed.samples[skip_samples..].to_vec()
        } else {
            mixed.samples
        };

        let initial_offset_secs = mixed.starts_at_secs_in_b + (skip_samples as f64 / bytes_per_sec);
        duration_secs_state.store(mixed.track_b_total_secs as u32, Ordering::Relaxed);
        progress_secs.store(initial_offset_secs as u32, Ordering::Relaxed);

        // Only now - right as we're about to actually start the new
        // stream - do we stop the old one. Everything above (download,
        // decode, analyze, mix) could take many seconds; the old track
        // keeps playing normally the whole time instead of going silent
        // while we work.
        if let Some(old_flag) = old_cancel_flag {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(180));
                old_flag.store(true, Ordering::SeqCst);
            });
        }

        // Play mixed buffer
        if let Err(e) = stream_player::play_pcm_buffer(
            samples_to_play,
            mixed.sample_rate,
            mixed.channels,
            initial_offset_secs,
            cancel_flag,
            volume,
            progress_secs,
            is_paused,
            eq_enabled,
            eq_gains,
            app_handle,
        ) {
            eprintln!("Playback error: {}", e);
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn get_cache_stats(cache: tauri::State<'_, CacheManager>) -> Result<CacheStats, String> {
    let cache = cache.inner().clone();
    tauri::async_runtime::spawn_blocking(move || cache.stats())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_cache(
    cache: tauri::State<'_, CacheManager>,
    keep_ids: Vec<String>,
) -> Result<CacheStats, String> {
    let cache = cache.inner().clone();
    tauri::async_runtime::spawn_blocking(move || cache.clear(&keep_ids))
        .await
        .map_err(|e| e.to_string())
}

