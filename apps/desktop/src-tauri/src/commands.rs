//! Typed Tauri IPC commands. Each command validates its own input and delegates to the
//! application facade, returning structured `AppError` values.

use tauri::State;
use world_application::{
    app_info as build_app_info, Actor, AppError, AppInfo, ChunkSnapshotBatch, HistoryState,
    HistorySummary, Operation, OperationRequest, OperationResult, PingResponse, PreviewResult,
    Query, QueryResult, RegionBlocks, RenderSnapshot, WorldService,
};
use world_domain::{BlockBox, BlockDefinition, BlockPos, BlockState, ChunkPos, WorldInfo};

use crate::state::{AppState, CurrentProject, SessionStatus};

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

// ---------------------------------------------------------------------------------------
// Project persistence (M5). The desktop shell owns the session; all file logic lives in
// `project_store`, and the authoritative world is still the single `WorldService`.
// ---------------------------------------------------------------------------------------

fn project_error(error: project_store::ProjectError) -> AppError {
    use world_application::ErrorCode;
    let code = match error.code() {
        "invalid_argument" => ErrorCode::InvalidArgument,
        "not_found" => ErrorCode::NotFound,
        "conflict" => ErrorCode::Conflict,
        "permission_denied" => ErrorCode::PermissionDenied,
        "limit_exceeded" => ErrorCode::LimitExceeded,
        "unsupported_version" => ErrorCode::UnsupportedVersion,
        "unsupported_format" => ErrorCode::UnsupportedFormat,
        _ => ErrorCode::Internal,
    };
    AppError::new(code, error.to_string())
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedProjectPayload {
    pub detail: project_store::ProjectDetail,
    pub info: WorldInfo,
    pub recovery: Option<project_store::RecoveryInfo>,
    pub session: SessionStatus,
}

fn current_payload(
    state: &AppState,
    detail: project_store::ProjectDetail,
    world_info: WorldInfo,
    recovery: Option<project_store::RecoveryInfo>,
) -> Result<OpenedProjectPayload, AppError> {
    let session = session_status(state)?;
    Ok(OpenedProjectPayload {
        detail,
        info: world_info,
        recovery,
        session,
    })
}

fn session_status(state: &AppState) -> Result<SessionStatus, AppError> {
    let revision = {
        let service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        service.world().revision()
    };
    let projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    let checkpoint_count = projects
        .current
        .as_ref()
        .and_then(|current| projects.checkpoints.get(&current.folder))
        .map(Vec::len)
        .unwrap_or(0);
    Ok(match &projects.current {
        Some(current) => SessionStatus {
            folder: Some(current.folder.clone()),
            name: Some(current.name.clone()),
            saved_revision: current.saved_revision,
            autosaved_revision: current.autosaved_revision,
            revision,
            dirty: revision != current.saved_revision,
            editor_state: Some(current.editor_state.clone()),
            available_checkpoints: checkpoint_count,
        },
        None => SessionStatus {
            folder: None,
            name: None,
            saved_revision: 0,
            autosaved_revision: None,
            revision,
            dirty: false,
            editor_state: None,
            available_checkpoints: 0,
        },
    })
}

#[tauri::command]
pub fn project_status(state: State<'_, AppState>) -> Result<SessionStatus, AppError> {
    session_status(&state)
}

#[tauri::command]
pub fn project_list(state: State<'_, AppState>) -> Result<project_store::ProjectList, AppError> {
    let projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    Ok(projects.store.list())
}

#[tauri::command]
pub fn project_create(
    state: State<'_, AppState>,
    name: String,
    mode: project_store::CreationMode,
    editor_state: project_store::EditorState,
) -> Result<OpenedProjectPayload, AppError> {
    let mut projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    let detail = projects
        .store
        .create(&name, mode, editor_state.clone())
        .map_err(project_error)?;
    let folder = detail.summary.folder.clone();
    let (detail, world, recovery) = projects.store.open(&folder).map_err(project_error)?;
    projects
        .store
        .begin_session(&folder, world.revision())
        .map_err(project_error)?;
    let info = world.info();
    {
        let mut service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        *service = WorldService::from_world(world);
    }
    projects.current = Some(CurrentProject {
        folder: folder.clone(),
        name: detail.summary.name.clone(),
        saved_revision: detail.summary.world_revision,
        autosaved_revision: None,
        editor_state,
    });
    projects.refresh_checkpoints(&folder);
    drop(projects);
    current_payload(&state, detail, info, recovery)
}

#[tauri::command]
pub fn project_open(
    state: State<'_, AppState>,
    folder: String,
) -> Result<OpenedProjectPayload, AppError> {
    let mut projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    if let Some(current) = &projects.current {
        if current.folder == folder {
            let revision = {
                let service = state
                    .service
                    .lock()
                    .map_err(|_| AppError::internal("world state lock poisoned"))?;
                service.world().revision()
            };
            if revision != current.saved_revision {
                return Err(AppError::new(
                    world_application::ErrorCode::Conflict,
                    "this project is already open with unsaved changes; save or close it first",
                ));
            }
        }
    }
    let (detail, world, recovery) = projects.store.open(&folder).map_err(project_error)?;
    projects
        .store
        .begin_session(&folder, world.revision())
        .map_err(project_error)?;
    let info = world.info();
    {
        let mut service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        *service = WorldService::from_world(world);
    }
    projects.current = Some(CurrentProject {
        folder: folder.clone(),
        name: detail.summary.name.clone(),
        saved_revision: detail.summary.world_revision,
        autosaved_revision: recovery.as_ref().map(|info| info.recovered_revision),
        editor_state: detail.editor_state.clone(),
    });
    projects.refresh_checkpoints(&folder);
    drop(projects);
    current_payload(&state, detail, info, recovery)
}

#[tauri::command]
pub fn project_save(
    state: State<'_, AppState>,
    editor_state: project_store::EditorState,
) -> Result<project_store::SaveOutcome, AppError> {
    let mut projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    let current = projects.current.clone().ok_or_else(|| {
        AppError::new(world_application::ErrorCode::NotFound, "no project is open")
    })?;
    editor_state.validate().map_err(project_error)?;
    let world = {
        let service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        service.world().clone()
    };
    let outcome = projects
        .store
        .save(
            &current.folder,
            &world,
            editor_state.clone(),
            project_store::SaveKind::Manual,
        )
        .map_err(project_error)?;
    if let Some(active) = projects.current.as_mut() {
        active.saved_revision = outcome.project.summary.world_revision;
        active.autosaved_revision = None;
        active.editor_state = editor_state;
    }
    projects.refresh_checkpoints(&current.folder);
    Ok(outcome)
}

#[tauri::command]
pub fn project_autosave(
    state: State<'_, AppState>,
    editor_state: project_store::EditorState,
    reason: Option<String>,
) -> Result<u64, AppError> {
    let mut projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    let current = projects.current.clone().ok_or_else(|| {
        AppError::new(world_application::ErrorCode::NotFound, "no project is open")
    })?;
    editor_state.validate().map_err(project_error)?;
    let world = {
        let service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        service.world().clone()
    };
    let revision = projects
        .store
        .autosave(
            &current.folder,
            &world,
            editor_state.clone(),
            reason.as_deref().unwrap_or(""),
        )
        .map_err(project_error)?;
    if let Some(active) = projects.current.as_mut() {
        active.autosaved_revision = Some(revision);
        active.editor_state = editor_state;
    }
    projects.refresh_checkpoints(&current.folder);
    Ok(revision)
}

#[tauri::command]
pub fn project_checkpoint(
    state: State<'_, AppState>,
    reason: String,
) -> Result<Vec<project_store::CheckpointMeta>, AppError> {
    let mut projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    let current = projects.current.clone().ok_or_else(|| {
        AppError::new(world_application::ErrorCode::NotFound, "no project is open")
    })?;
    let world = {
        let service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        service.world().clone()
    };
    projects
        .store
        .checkpoint(
            &current.folder,
            &world,
            current.editor_state.clone(),
            &reason,
        )
        .map_err(project_error)?;
    projects.refresh_checkpoints(&current.folder);
    projects
        .store
        .checkpoints(&current.folder)
        .map_err(project_error)
}

#[tauri::command]
pub fn project_checkpoints(
    state: State<'_, AppState>,
    folder: Option<String>,
) -> Result<Vec<project_store::CheckpointMeta>, AppError> {
    let projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    let folder = folder
        .or_else(|| {
            projects
                .current
                .as_ref()
                .map(|current| current.folder.clone())
        })
        .ok_or_else(|| {
            AppError::new(world_application::ErrorCode::NotFound, "no project is open")
        })?;
    projects.store.checkpoints(&folder).map_err(project_error)
}

#[tauri::command]
pub fn project_open_checkpoint(
    state: State<'_, AppState>,
    folder: Option<String>,
    revision: u64,
) -> Result<WorldInfo, AppError> {
    let mut projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    let folder = folder
        .or_else(|| {
            projects
                .current
                .as_ref()
                .map(|current| current.folder.clone())
        })
        .ok_or_else(|| {
            AppError::new(world_application::ErrorCode::NotFound, "no project is open")
        })?;
    let world = projects
        .store
        .open_checkpoint(&folder, revision)
        .map_err(project_error)?;
    let info = world.info();
    {
        let mut service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        *service = WorldService::from_world(world);
    }
    if let Some(current) = projects.current.as_mut() {
        if current.folder == folder {
            current.autosaved_revision = Some(revision);
        }
    }
    drop(projects);
    let _ = session_status(&state)?;
    Ok(info)
}

#[tauri::command]
pub fn project_close(
    state: State<'_, AppState>,
    save: bool,
    editor_state: project_store::EditorState,
) -> Result<SessionStatus, AppError> {
    let mut projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    let Some(current) = projects.current.clone() else {
        drop(projects);
        return session_status(&state);
    };
    if save {
        editor_state.validate().map_err(project_error)?;
        let world = {
            let service = state
                .service
                .lock()
                .map_err(|_| AppError::internal("world state lock poisoned"))?;
            service.world().clone()
        };
        projects
            .store
            .save(
                &current.folder,
                &world,
                editor_state,
                project_store::SaveKind::Manual,
            )
            .map_err(project_error)?;
    } else {
        // Explicit discard: the user chose to drop unsaved work, so crash-recovery
        // artifacts must not resurrect it on the next open.
        projects
            .store
            .clear_autosave(&current.folder)
            .map_err(project_error)?;
    }
    projects
        .store
        .end_session(&current.folder)
        .map_err(project_error)?;
    projects.current = None;
    {
        let mut service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        *service = WorldService::new();
    }
    drop(projects);
    session_status(&state)
}

#[tauri::command]
pub fn project_rename(
    state: State<'_, AppState>,
    folder: String,
    name: String,
) -> Result<project_store::ProjectDetail, AppError> {
    let mut projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    let detail = projects
        .store
        .rename(&folder, &name)
        .map_err(project_error)?;
    if let Some(current) = projects.current.as_mut() {
        if current.folder == folder {
            current.name = detail.summary.name.clone();
        }
    }
    Ok(detail)
}
