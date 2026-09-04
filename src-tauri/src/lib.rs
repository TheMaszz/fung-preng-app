pub mod analyze;
pub mod cache;
pub mod decode;
pub mod extract;
pub mod mixer;
pub mod output;
pub mod playback;
pub mod resample;
pub mod search;

use search::{search_youtube, youtube_suggestions};

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            greet,
            playback::play_track,
            search_youtube,
            youtube_suggestions
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}