// src-tauri/src/cache.rs
use crate::analyze::AudioAnalysis;
use anyhow::{Context, Result};
use std::fs::{self, File};
use std::io::{Write};
use std::path::{Path, PathBuf};

pub struct CacheManager {
    cache_dir: PathBuf,
}

impl CacheManager {
    pub fn new<P: AsRef<Path>>(cache_dir: P) -> Result<Self> {
        let dir = cache_dir.as_ref().to_path_buf();
        if !dir.exists() {
            fs::create_dir_all(&dir)
                .with_context(|| format!("Failed to create cache directory: {:?}", dir))?;
        }
        Ok(Self { cache_dir: dir })
    }

    /// Default system temp cache path: ./cache
    pub fn default_dir() -> Result<Self> {
        Self::new(PathBuf::from("./cache"))
    }

    /// Returns the cached audio file path if it exists
    pub fn get_audio_path(&self, video_id: &str) -> Option<PathBuf> {
        let path = self.cache_dir.join(format!("{}.wav", video_id));
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    /// Saves or retrieves analysis metadata for a given video ID
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
