use std::sync::{
    atomic::AtomicBool,
    Arc, Mutex,
};
use serde::Serialize;

#[derive(Clone, Copy, Serialize)]
pub struct PlaybackProgress {
    pub position_secs: f64,
    pub duration_secs: f64,
}

pub struct AppPlayerState {
    pub cancel_flag: Mutex<Arc<AtomicBool>>,
    pub is_paused: Arc<AtomicBool>,
    pub volume: Arc<Mutex<f32>>,
    pub progress: Arc<Mutex<PlaybackProgress>>,
}

impl Default for AppPlayerState {
    fn default() -> Self {
        Self {
            cancel_flag: Mutex::new(Arc::new(AtomicBool::new(false))),
            is_paused: Arc::new(AtomicBool::new(false)),
            volume: Arc::new(Mutex::new(0.8)),
            progress: Arc::new(Mutex::new(PlaybackProgress {
                position_secs: 0.0,
                duration_secs: 0.0,
            })),
        }
    }
}