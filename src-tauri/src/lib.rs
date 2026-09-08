pub mod analyze;
pub mod cache;
pub mod decode;
pub mod extract;
pub mod mixer;
pub mod output;
pub mod playback;
pub mod resample;
pub mod search;
pub mod stream_source;
pub mod stream_player;
pub mod player_state;
pub mod commands;

use player_state::AppPlayerState;
use crate::cache::CacheManager;

use search::{search_youtube, youtube_suggestions};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cache_manager = CacheManager::default_dir().expect("failed to init cache dir");
    tauri::Builder::default()
        .manage(AppPlayerState::default())
        .manage(cache_manager)
        .invoke_handler(tauri::generate_handler![
            playback::play_track,
            search_youtube,
            youtube_suggestions,
            commands::play_audio,
            commands::pause_audio,  
            commands::resume_audio,
            commands::resolve_audio_url,
            commands::stop_audio,
            commands::set_volume,
            commands::get_playback_progress
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}