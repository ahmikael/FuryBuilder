use std::sync::Mutex;

use world_application::WorldService;

/// Shared authoritative world service for the Tauri process.
pub struct AppState {
    pub service: Mutex<WorldService>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            service: Mutex::new(WorldService::new()),
        }
    }
}
