#[tauri::command]
pub async fn play_track(video_id: String) -> Result<(), String> {
    let path = crate::extract::extract_audio(&video_id)
        .await
        .map_err(|error| error.to_string())?;
    let playback_path = path.clone();

    let playback_result = tokio::task::spawn_blocking(move || {
        let audio =
            crate::decode::decode_to_pcm(&playback_path).map_err(|error| error.to_string())?;
        crate::output::play_blocking(&audio).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("playback task failed: {error}"))?;

    std::fs::remove_file(&path)
        .map_err(|error| format!("failed to remove temporary audio file: {error}"))?;

    playback_result
}
