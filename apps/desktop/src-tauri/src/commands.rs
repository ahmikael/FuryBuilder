//! Typed Tauri IPC commands. Each command validates its own input and delegates to the
//! application facade, returning structured `AppError` values.

use tauri::State;
use world_application::{
    app_info as build_app_info, AppError, AppInfo, ChunkSnapshotBatch, PingResponse, RenderSnapshot,
};
use world_domain::{BlockPos, BlockState, ChunkPos, WorldInfo};

use crate::state::AppState;

#[tauri::command]
pub fn app_info() -> AppInfo {
    build_app_info()
}

#[tauri::command]
pub fn ping(name: Option<String>) -> Result<PingResponse, AppError> {
    world_application::ping(name.as_deref())
}

#[tauri::command]
pub fn world_info(state: State<'_, AppState>) -> Result<WorldInfo, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    Ok(service.world_info())
}

#[tauri::command]
pub fn load_demo_world(state: State<'_, AppState>) -> Result<WorldInfo, AppError> {
    let mut service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    service.load_demo_world()
}

#[tauri::command]
pub fn render_snapshot(
    state: State<'_, AppState>,
    dimension: String,
    center: ChunkPos,
    radius: u32,
) -> Result<RenderSnapshot, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    service.render_snapshot(&dimension, center, radius)
}

#[tauri::command]
pub fn render_chunks(
    state: State<'_, AppState>,
    dimension: String,
    chunks: Vec<ChunkPos>,
) -> Result<ChunkSnapshotBatch, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    service.render_chunks(&dimension, &chunks)
}

#[tauri::command]
pub fn get_block(
    state: State<'_, AppState>,
    dimension: String,
    pos: BlockPos,
) -> Result<BlockState, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    Ok(service.world().get_block(&dimension, pos)?)
}
