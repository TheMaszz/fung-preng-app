pub mod analyze;
pub mod cache;
pub mod commands;
pub mod database;
pub mod decode;
pub mod extract;
pub mod mixer;
pub mod output;
pub mod playback;
pub mod player_state;
pub mod resample;
pub mod search;
pub mod stream_player;
pub mod stream_source;

// use player_state::AppPlayerState;
use crate::cache::CacheManager;
use crate::database::AppDatabase;
use commands::AppState;
use tauri::Manager;

use search::{search_youtube, youtube_suggestions};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cache_manager = CacheManager::default_dir().expect("failed to init cache dir");
    tauri::Builder::default()
        .manage(AppState::default())
        .manage(cache_manager)
        .setup(|app| {
            let bundled_ytdlp = app
                .path()
                .resolve("binaries/yt-dlp.exe", tauri::path::BaseDirectory::Resource)?;
            let development_ytdlp = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("binaries")
                .join("yt-dlp.exe");
            let ytdlp = if bundled_ytdlp.is_file() {
                bundled_ytdlp
            } else if development_ytdlp.is_file() {
                development_ytdlp
            } else {
                std::path::PathBuf::from("yt-dlp")
            };
            extract::configure_ytdlp_executable(ytdlp);

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let database = AppDatabase::open(data_dir.join("fung-pleng.sqlite"))
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
            app.manage(database);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            playback::play_track,
            search_youtube,
            youtube_suggestions,
            commands::play_audio,
            commands::record_listen_history,
            commands::import_legacy_listen_history,
            commands::get_listen_history,
            commands::clear_listen_history,
            commands::get_playlists,
            commands::create_playlist,
            commands::delete_playlist,
            commands::add_playlist_track,
            commands::add_track_to_playlist,
            commands::get_playlist_tracks,
            commands::is_track_in_any_playlist,
            commands::reorder_playlist_tracks,
            commands::remove_track_from_playlist,
            commands::pause_audio,
            commands::resume_audio,
            commands::resolve_audio_url,
            commands::get_track_duration,
            commands::stop_audio,
            commands::set_volume,
            commands::set_eq,
            commands::get_playback_progress,
            commands::get_related_tracks,
            commands::play_mixed_session,
            commands::prefetch_track_urls,
            commands::prepare_mixed_session,
            commands::get_cache_stats,
            commands::clear_cache,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
