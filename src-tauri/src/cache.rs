// src-tauri/src/cache.rs
use crate::analyze::AudioAnalysis;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

pub struct UrlCacheEntry {
    pub url: String,
    pub fetched_at: Instant,
}

#[derive(Clone)]
pub struct CacheManager {
    cache_dir: PathBuf,
    url_cache: Arc<RwLock<HashMap<String, UrlCacheEntry>>>,
}

impl CacheManager {
    pub fn new<P: AsRef<Path>>(cache_dir: P) -> Result<Self> {
        let dir = cache_dir.as_ref().to_path_buf();
        if !dir.exists() {
            fs::create_dir_all(&dir)
                .with_context(|| format!("Failed to create cache directory: {:?}", dir))?;
        }
        Ok(Self {
            cache_dir: dir,
            url_cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub fn default_dir() -> Result<Self> {
        Self::new(PathBuf::from("./cache"))
    }

    pub fn get_cached_url(&self, video_id: &str) -> Option<String> {
        let cache = self.url_cache.read().ok()?;
        if let Some(entry) = cache.get(video_id) {
            if entry.fetched_at.elapsed() < Duration::from_secs(4 * 3600) {
                return Some(entry.url.clone());
            }
        }
        None
    }

    pub fn save_url_cache(&self, video_id: String, url: String) {
        if let Ok(mut cache) = self.url_cache.write() {
            cache.insert(
                video_id,
                UrlCacheEntry {
                    url,
                    fetched_at: Instant::now(),
                },
            );
        }
    }

    pub fn get_audio_path(&self, video_id: &str) -> Option<PathBuf> {
        let path = self.cache_dir.join(format!("{}.wav", video_id));
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    pub fn get_analysis(&self, video_id: &str) -> Option<AudioAnalysis> {
        let path = self.cache_dir.join(format!("{}.json", video_id));
        let json_str = fs::read_to_string(path).ok()?;
        serde_json::from_str(&json_str).ok()
    }

    pub fn save_analysis(&self, video_id: &str, analysis: &AudioAnalysis) -> Result<()> {
        let path = self.cache_dir.join(format!("{}.json", video_id));
        let json_str = serde_json::to_string_pretty(analysis)?;
        let mut file = File::create(path)?;
        file.write_all(json_str.as_bytes())?;
        Ok(())
    }
}