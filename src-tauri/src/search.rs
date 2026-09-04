use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub channel: String,
    pub duration_secs: f64,
    pub thumbnail_url: String,
}

#[tauri::command]
pub async fn youtube_suggestions(query: String) -> Result<Vec<String>, String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let client = reqwest::Client::new();
    let response = client
        .get("https://suggestqueries.google.com/complete/search")
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
        )
        .query(&[("client", "firefox"), ("ds", "yt"), ("q", query.as_str())])
        .send()
        .await
        .map_err(|error| format!("failed to fetch YouTube suggestions: {error}"))?
        .error_for_status()
        .map_err(|error| format!("YouTube suggestions returned an error: {error}"))?
        .json::<serde_json::Value>()
        .await
        .map_err(|error| format!("failed to parse YouTube suggestions: {error}"))?;

    let suggestions = response
        .get(1)
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .take(8)
                .collect()
        })
        .unwrap_or_default();

    Ok(suggestions)
}

#[tauri::command]
pub async fn search_youtube(query: String, offset: usize) -> Result<Vec<SearchResult>, String> {
    tokio::task::spawn_blocking(move || {
        const PAGE_SIZE: usize = 10;
        let requested_results = offset.saturating_add(PAGE_SIZE);
        let output = Command::new("yt-dlp")
            .args([
                &format!("ytsearch{}:{}", requested_results, query),
                "--dump-json",
                "--flat-playlist",
                "--default-search", "ytsearch",
            ])
            .output()
            .map_err(|e| e.to_string())?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut results = Vec::new();

        for line in stdout.lines().skip(offset).take(PAGE_SIZE) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                let id = v["id"].as_str().unwrap_or_default().to_string();
                let title = v["title"].as_str().unwrap_or("Unknown").to_string();
                let channel = v["uploader"].as_str().unwrap_or("Unknown").to_string();
                let duration_secs = v["duration"].as_f64().unwrap_or(0.0);
                let thumbnail_url = format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", id);

                results.push(SearchResult {
                    id,
                    title,
                    channel,
                    duration_secs,
                    thumbnail_url,
                });
            }
        }
        Ok(results)
    })
    .await
    .map_err(|e| e.to_string())?
}