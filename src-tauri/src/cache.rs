// src-tauri/src/cache.rs
use crate::analyze::AudioAnalysis;
use anyhow::{Context, Result};
use serde::Serialize;
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

/// Sizes in bytes, split by what the files actually are on disk.
#[derive(Serialize, Clone, Default)]
pub struct CacheStats {
    pub songs: u64,    // *.wav  (decoded audio)
    pub analysis: u64, // *.json (AudioAnalysis)
    pub temp: u64,     // anything else (.part, .tmp, leftovers)
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

    /// Prefer this over `default_dir()`: a real, stable OS location
    /// (and it won't trigger Tauri dev-mode rebuilds like ./cache can).
    pub fn from_app(app: &tauri::AppHandle) -> Result<Self> {
        use tauri::Manager;
        let dir = app.path().app_cache_dir()?.join("audio");
        Self::new(dir)
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

    // ---------------------------------------------------------------
    // NEW: real disk usage + clearing
    // ---------------------------------------------------------------

    /// Walks the cache dir and sums real file sizes by type.
    pub fn stats(&self) -> CacheStats {
        let mut s = CacheStats::default();
        let Ok(rd) = fs::read_dir(&self.cache_dir) else {
            return s;
        };
        for entry in rd.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let len = meta.len();
            match entry.path().extension().and_then(|e| e.to_str()) {
                Some("wav") => s.songs += len,
                Some("json") => s.analysis += len,
                _ => s.temp += len,
            }
        }
        s
    }

    /// Deletes cached files. Files that can't be removed (e.g. the track
    /// currently playing, on Windows) are skipped rather than failing.
    /// `keep_ids` lets the caller protect specific tracks, e.g. current + next.
    pub fn clear(&self, keep_ids: &[String]) -> CacheStats {
        if let Ok(rd) = fs::read_dir(&self.cache_dir) {
            for entry in rd.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if keep_ids.iter().any(|k| k == stem) {
                    continue;
                }
                let _ = fs::remove_file(&path);
            }
        }
        if let Ok(mut c) = self.url_cache.write() {
            c.clear();
        }
        self.stats()
    }
}