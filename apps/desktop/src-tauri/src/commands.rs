//! Typed Tauri IPC commands. Each command validates its own input and delegates to the
//! application facade, returning structured `AppError` values.

use tauri::State;
use world_application::{
    app_info as build_app_info, Actor, AppError, AppInfo, ChunkSnapshotBatch, HistoryState,
    HistorySummary, Operation, OperationRequest, OperationResult, PingResponse, PreviewResult,
    Query, QueryResult, RegionBlocks, RenderSnapshot,
};
use world_domain::{BlockBox, BlockDefinition, BlockPos, BlockState, ChunkPos, WorldInfo};

use crate::state::AppState;

const UI_PROJECT_ID: &str = "local-project";

fn ui_actor() -> Actor {
    Actor::ui("ui")
}

#[tauri::command]
pub fn block_catalog(state: State<'_, AppState>) -> Result<Vec<BlockDefinition>, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    Ok(service.registry().definitions())
}

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
pub fn preview_operation(
    state: State<'_, AppState>,
    operation: Operation,
) -> Result<PreviewResult, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    service.preview(UI_PROJECT_ID, &operation)
}

#[tauri::command]
pub fn apply_operation(
    state: State<'_, AppState>,
    operation: Operation,
    expected_revision: Option<u64>,
    idempotency_key: Option<String>,
) -> Result<OperationResult, AppError> {
    let mut service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    service.execute(OperationRequest {
        project_id: UI_PROJECT_ID.to_owned(),
        operation,
        expected_revision,
        idempotency_key,
        actor: ui_actor(),
        linked_object_id: None,
    })
}

#[tauri::command]
pub fn undo_operation(
    state: State<'_, AppState>,
    operation_id: Option<String>,
) -> Result<OperationResult, AppError> {
    let mut service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    service.undo(UI_PROJECT_ID, ui_actor(), operation_id.as_deref())
}

#[tauri::command]
pub fn redo_operation(state: State<'_, AppState>) -> Result<OperationResult, AppError> {
    let mut service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    service.redo(UI_PROJECT_ID, ui_actor())
}

#[tauri::command]
pub fn operation_history(state: State<'_, AppState>) -> Result<Vec<HistorySummary>, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    Ok(service.history())
}

#[tauri::command]
pub fn history_state(state: State<'_, AppState>) -> Result<HistoryState, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    Ok(service.history_state())
}

#[tauri::command]
pub fn read_region(
    state: State<'_, AppState>,
    dimension: String,
    bounds: BlockBox,
) -> Result<RegionBlocks, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    match service.query(Query::RegionBlocks { dimension, bounds })? {
        QueryResult::RegionBlocks(region) => Ok(region),
        _ => Err(AppError::internal(
            "unexpected query result for region blocks",
        )),
    }
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
