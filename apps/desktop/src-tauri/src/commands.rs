//! Typed Tauri IPC commands. Each command validates its own input and delegates to the
//! application facade, returning structured `AppError` values.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use tauri::State;
use world_application::{
    app_info as build_app_info, Actor, AppError, AppInfo, BlockChange, ChunkSnapshotBatch,
    ErrorCode, HistoryState, HistorySummary, Operation, OperationRequest, OperationResult,
    PingResponse, PreviewResult, Query, QueryResult, RegionBlocks, RenderSnapshot, WorldService,
};
use world_domain::{BlockBox, BlockDefinition, BlockPos, BlockState, ChunkPos, World, WorldInfo};
use world_io::{self, JavaStructureError, PaletteCount, Structure, StructureImportPreview};

use crate::state::{
    session_status, AppState, CurrentProject, ProjectSession, SessionStatus, LOCAL_PROJECT_ID,
};

/// Compressed structure uploads are capped before base64 decoding; the NBT codec then caps
/// decompressed size and every collection independently.
const MAX_STRUCTURE_UPLOAD_BYTES: usize = 32 * 1024 * 1024;

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
    service.preview(LOCAL_PROJECT_ID, &operation)
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
        project_id: LOCAL_PROJECT_ID.to_owned(),
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
    service.undo(LOCAL_PROJECT_ID, ui_actor(), operation_id.as_deref())
}

#[tauri::command]
pub fn redo_operation(state: State<'_, AppState>) -> Result<OperationResult, AppError> {
    let mut service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    service.redo(LOCAL_PROJECT_ID, ui_actor())
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

#[tauri::command]
pub fn project_status(state: State<'_, AppState>) -> Result<SessionStatus, AppError> {
    session_status(&state)
}

/// Ends a clean open session before switching projects. Unsaved work is never discarded
/// implicitly: the caller must save or close it first.
fn ensure_current_closed(state: &AppState, projects: &mut ProjectSession) -> Result<(), AppError> {
    let Some(current) = projects.current.clone() else {
        return Ok(());
    };
    let revision = {
        let service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        service.world().revision()
    };
    if revision != current.saved_revision {
        return Err(AppError::new(
            ErrorCode::Conflict,
            "the open project has unsaved changes; save or discard them before switching projects",
        ));
    }
    projects
        .store
        .end_session(&current.folder)
        .map_err(project_error)?;
    projects.current = None;
    Ok(())
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
    ensure_current_closed(&state, &mut projects)?;
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
        project_id: detail.summary.project_id.clone(),
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
    ensure_current_closed(&state, &mut projects)?;
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
        project_id: detail.summary.project_id.clone(),
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

// ---------------------------------------------------------------------------------------
// Structure import/export (M6). Format logic lives in `world_io`; this layer only moves
// bytes, enforces project/session policy and publishes files through `project_store`.
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureExportPreview {
    pub format: String,
    pub data_version: i32,
    pub target_data_version: i32,
    pub target_minecraft_version: String,
    pub dimension: String,
    pub bounds: BlockBox,
    pub size: [i32; 3],
    pub volume: u64,
    pub palette_size: usize,
    pub palette: Vec<PaletteCount>,
    pub non_air_blocks: usize,
    pub stored_blocks: usize,
    pub bytes: u64,
    pub checksum: String,
    pub file_name: String,
    pub relative_path: String,
    pub warnings: Vec<String>,
    pub compatible: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureExportResult {
    pub outcome: project_store::ExportOutcome,
    pub verified: bool,
    pub size: [i32; 3],
    pub non_air_blocks: usize,
    pub data_version: i32,
    pub target_data_version: i32,
    pub target_minecraft_version: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureInsertOutcome {
    pub preview: StructureImportPreview,
    pub operation: Option<OperationResult>,
    pub info: WorldInfo,
    pub session: SessionStatus,
}

fn structure_error(error: JavaStructureError) -> AppError {
    let code = match error.code() {
        "unsupported_format" => ErrorCode::UnsupportedFormat,
        "unsupported_version" => ErrorCode::UnsupportedVersion,
        "limit_exceeded" => ErrorCode::LimitExceeded,
        "invalid_argument" => ErrorCode::InvalidArgument,
        _ => ErrorCode::Internal,
    };
    AppError::new(code, error.to_string())
}

fn structure_modification_error(error: world_io::StructureError) -> AppError {
    let code = if matches!(
        &error,
        world_io::StructureError::TooLarge { .. } | world_io::StructureError::TooManyBlocks { .. }
    ) {
        ErrorCode::LimitExceeded
    } else {
        ErrorCode::InvalidArgument
    };
    AppError::new(code, error.to_string())
}

fn decode_structure_payload(payload: &str) -> Result<Vec<u8>, AppError> {
    if payload.len() > MAX_STRUCTURE_UPLOAD_BYTES.saturating_mul(4) / 3 + 8 {
        return Err(AppError::new(
            ErrorCode::LimitExceeded,
            format!("structure upload exceeds {MAX_STRUCTURE_UPLOAD_BYTES} bytes"),
        ));
    }
    let bytes = BASE64.decode(payload).map_err(|_| {
        AppError::new(
            ErrorCode::InvalidArgument,
            "structure payload is not valid base64",
        )
    })?;
    if bytes.len() > MAX_STRUCTURE_UPLOAD_BYTES {
        return Err(AppError::new(
            ErrorCode::LimitExceeded,
            format!("structure upload exceeds {MAX_STRUCTURE_UPLOAD_BYTES} bytes"),
        ));
    }
    Ok(bytes)
}

fn sanitize_import_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let mut cleaned: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    while cleaned.contains("--") {
        cleaned = cleaned.replace("--", "-");
    }
    let mut cleaned = cleaned.trim_matches('-').to_owned();
    if cleaned.len() > 96 {
        cleaned.truncate(96);
    }
    if cleaned.is_empty() {
        cleaned = "structure.nbt".to_owned();
    } else if !cleaned.to_ascii_lowercase().ends_with(".nbt") {
        cleaned.push_str(".nbt");
    }
    cleaned
}

fn export_stem(name: &str) -> String {
    let mut stem: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while stem.contains("--") {
        stem = stem.replace("--", "-");
    }
    let stem: String = stem.trim_matches('-').chars().take(48).collect();
    if stem.is_empty() {
        "structure".to_owned()
    } else {
        stem
    }
}

fn dimension_short(dimension: &str) -> String {
    let short = dimension.rsplit(':').next().unwrap_or(dimension);
    let cleaned: String = short
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('-');
    if cleaned.is_empty() {
        "world".to_owned()
    } else {
        cleaned.to_owned()
    }
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn placement_origin(
    dimension: &str,
    origin: [i32; 3],
    size: [i32; 3],
) -> Result<BlockPos, AppError> {
    let world = World::new_empty();
    let target = world.dimension(dimension).map_err(|_| {
        AppError::new(
            ErrorCode::InvalidArgument,
            format!("unknown dimension `{dimension}`"),
        )
    })?;
    if size.iter().any(|axis| *axis <= 0) {
        return Err(AppError::new(
            ErrorCode::InvalidArgument,
            "structure size must be positive on every axis",
        ));
    }
    let last_y = origin[1].checked_add(size[1] - 1).ok_or_else(|| {
        AppError::new(
            ErrorCode::InvalidArgument,
            "structure origin overflows the vertical coordinate",
        )
    })?;
    if origin[1] < target.min_y() || last_y > target.max_y() {
        return Err(AppError::new(
            ErrorCode::InvalidArgument,
            format!(
                "structure height {} at y={} does not fit in `{dimension}` (y {}..={})",
                size[1],
                origin[1],
                target.min_y(),
                target.max_y()
            ),
        ));
    }
    Ok(BlockPos::new(origin[0], origin[1], origin[2]))
}

fn import_changes(
    structure: &Structure,
    origin: BlockPos,
    include_air: bool,
) -> Result<Vec<BlockChange>, AppError> {
    structure
        .to_world_changes(origin, include_air)
        .map(|changes| {
            changes
                .into_iter()
                .map(|(pos, state)| BlockChange { pos, state })
                .collect()
        })
        .map_err(structure_modification_error)
}

#[tauri::command]
pub fn structure_import_preview(payload: String) -> Result<StructureImportPreview, AppError> {
    let bytes = decode_structure_payload(&payload)?;
    world_io::import_preview(&bytes).map_err(structure_error)
}

#[tauri::command]
// Tauri exposes flat command arguments; grouping them would change the public IPC shape
// without removing any validation. Keep the adapter explicit and satisfy the lint.
#[allow(clippy::too_many_arguments)]
pub fn structure_import_project(
    state: State<'_, AppState>,
    name: String,
    source_file_name: String,
    payload: String,
    dimension: String,
    origin: [i32; 3],
    include_air: bool,
    mut editor_state: project_store::EditorState,
) -> Result<OpenedProjectPayload, AppError> {
    let bytes = decode_structure_payload(&payload)?;
    let parsed = world_io::read_verified_import(&bytes).map_err(structure_error)?;
    let preview = world_io::preview_from_structure(&parsed.structure, &parsed.inspection);
    if !preview.compatible {
        return Err(AppError::new(
            ErrorCode::UnsupportedFormat,
            preview.warnings.join("; "),
        ));
    }
    let origin = placement_origin(&dimension, origin, preview.size)?;
    let changes = import_changes(&parsed.structure, origin, include_air)?;
    editor_state.selected_dimension = dimension.clone();
    editor_state.validate().map_err(project_error)?;

    let mut projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    ensure_current_closed(&state, &mut projects)?;
    let detail = projects
        .store
        .create(
            &name,
            project_store::CreationMode::Empty,
            editor_state.clone(),
        )
        .map_err(project_error)?;
    let folder = detail.summary.folder.clone();
    let source_name = sanitize_import_name(&source_file_name);
    let root = projects.store.root().to_path_buf();

    let built = (|| -> Result<(project_store::ProjectDetail, WorldInfo), AppError> {
        let (_, world, _) = projects.store.open(&folder).map_err(project_error)?;
        projects
            .store
            .attach_source_bytes(&folder, &source_name, &bytes)
            .map_err(project_error)?;
        let mut service = WorldService::from_world(world);
        // Imported structures may reference vanilla states outside the built-in palette
        // catalogue. They are preserved verbatim in the world; the catalogue is a UI
        // convenience, not the authoritative block list.
        service.set_allow_unknown_targets(true);
        if !changes.is_empty() {
            service.execute(OperationRequest {
                project_id: detail.summary.project_id.clone(),
                operation: Operation::Batch {
                    dimension: dimension.clone(),
                    changes,
                },
                expected_revision: None,
                idempotency_key: None,
                actor: ui_actor(),
                linked_object_id: None,
            })?;
        }
        // Unknown states are accepted only for the imported payload. Leaving this flag on
        // would let later commands write arbitrary block ids through this service.
        service.set_allow_unknown_targets(false);
        let outcome = projects
            .store
            .save(
                &folder,
                service.world(),
                editor_state.clone(),
                project_store::SaveKind::Manual,
            )
            .map_err(project_error)?;
        projects
            .store
            .begin_session(&folder, service.world().revision())
            .map_err(project_error)?;
        let info = service.world_info();
        {
            let mut current_service = state
                .service
                .lock()
                .map_err(|_| AppError::internal("world state lock poisoned"))?;
            *current_service = service;
        }
        projects.current = Some(CurrentProject {
            folder: folder.clone(),
            project_id: detail.summary.project_id.clone(),
            name: outcome.project.summary.name.clone(),
            saved_revision: outcome.project.summary.world_revision,
            autosaved_revision: None,
            editor_state: editor_state.clone(),
        });
        projects.refresh_checkpoints(&folder);
        Ok((outcome.project, info))
    })();

    match built {
        Ok((detail, info)) => {
            drop(projects);
            current_payload(&state, detail, info, None)
        }
        Err(error) => {
            projects.current = None;
            if let Ok(mut service) = state.service.lock() {
                *service = WorldService::new();
            }
            // Remove the half-created project folder: it was created by this command and
            // never exposed as an open project.
            let _ = std::fs::remove_dir_all(root.join(&folder));
            Err(error)
        }
    }
}

#[tauri::command]
pub fn structure_insert(
    state: State<'_, AppState>,
    payload: String,
    dimension: String,
    origin: [i32; 3],
    include_air: bool,
) -> Result<StructureInsertOutcome, AppError> {
    let bytes = decode_structure_payload(&payload)?;
    let parsed = world_io::read_verified_import(&bytes).map_err(structure_error)?;
    let preview = world_io::preview_from_structure(&parsed.structure, &parsed.inspection);
    if !preview.compatible {
        return Err(AppError::new(
            ErrorCode::UnsupportedFormat,
            preview.warnings.join("; "),
        ));
    }
    let origin = placement_origin(&dimension, origin, preview.size)?;
    let changes = import_changes(&parsed.structure, origin, include_air)?;

    let projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    let current = projects.current.clone().ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "no project is open; create a project before inserting a structure",
        )
    })?;
    let (operation, info) = {
        let mut service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        let operation = if changes.is_empty() {
            None
        } else {
            service.set_allow_unknown_targets(true);
            let result = service.execute(OperationRequest {
                project_id: current.project_id.clone(),
                operation: Operation::Batch {
                    dimension: dimension.clone(),
                    changes,
                },
                expected_revision: None,
                idempotency_key: None,
                actor: ui_actor(),
                linked_object_id: None,
            });
            service.set_allow_unknown_targets(false);
            Some(result?)
        };
        (operation, service.world_info())
    };
    drop(projects);
    let session = session_status(&state)?;
    Ok(StructureInsertOutcome {
        preview,
        operation,
        info,
        session,
    })
}

#[tauri::command]
pub fn structure_export_preview(
    state: State<'_, AppState>,
    dimension: String,
    bounds: Option<BlockBox>,
) -> Result<StructureExportPreview, AppError> {
    let current = {
        let projects = state
            .projects
            .lock()
            .map_err(|_| AppError::internal("project state lock poisoned"))?;
        projects.current.clone().ok_or_else(|| {
            AppError::new(
                ErrorCode::NotFound,
                "no project is open; create or open a project before exporting",
            )
        })?
    };
    let (bundle, revision, checksum) = {
        let service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        let bundle = world_io::prepare_export(service.world(), &dimension, bounds, "FuryBuilder")
            .map_err(structure_error)?;
        let revision = service.world().revision();
        let checksum = project_store::checksum(&bundle.bytes);
        (bundle, revision, checksum)
    };
    let mut warnings = bundle.warnings.clone();
    if revision != current.saved_revision {
        warnings.push(format!(
            "The export uses the current unsaved world revision {revision}; save the project to keep this state."
        ));
    }
    let file_name = format!(
        "{}-{}-r{}-{}.nbt",
        export_stem(&current.name),
        dimension_short(&dimension),
        revision,
        now_seconds()
    );
    Ok(StructureExportPreview {
        format: world_io::JAVA_STRUCTURE_FORMAT.to_owned(),
        data_version: bundle.inspection.data_version,
        target_data_version: bundle.inspection.target_data_version,
        target_minecraft_version: world_domain::TARGET_MINECRAFT_VERSION.to_owned(),
        dimension,
        bounds: bundle.bounds,
        size: bundle.inspection.size,
        volume: bundle.inspection.volume,
        palette_size: bundle.inspection.palette_size,
        palette: world_io::palette_counts(&bundle.structure),
        non_air_blocks: bundle.inspection.non_air_blocks,
        stored_blocks: bundle.inspection.stored_blocks,
        bytes: bundle.bytes.len() as u64,
        checksum,
        relative_path: format!(
            "{EXPORTS_DIR}/{file_name}",
            EXPORTS_DIR = project_store::EXPORTS_DIR
        ),
        file_name,
        warnings,
        compatible: true,
    })
}

#[tauri::command]
pub fn structure_export(
    state: State<'_, AppState>,
    dimension: String,
    bounds: Option<BlockBox>,
    file_name: String,
    expected_checksum: String,
) -> Result<StructureExportResult, AppError> {
    let projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    let current = projects.current.clone().ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "no project is open; create or open a project before exporting",
        )
    })?;
    if !file_name.to_ascii_lowercase().ends_with(".nbt") {
        return Err(AppError::new(
            ErrorCode::InvalidArgument,
            "this adapter can only export Java structure `.nbt` files",
        ));
    }
    let (bundle, checksum) = {
        let service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        let bundle = world_io::prepare_export(service.world(), &dimension, bounds, "FuryBuilder")
            .map_err(structure_error)?;
        let checksum = project_store::checksum(&bundle.bytes);
        (bundle, checksum)
    };
    if expected_checksum != checksum {
        return Err(AppError::new(
            ErrorCode::Conflict,
            "the world changed since the export preview; refresh the preview and try again",
        ));
    }
    let outcome = projects
        .store
        .write_export(&current.folder, &file_name, &bundle.bytes)
        .map_err(project_error)?;
    let path = projects
        .store
        .root()
        .join(&current.folder)
        .join(&outcome.relative_path);
    let written = std::fs::read(&path).map_err(|error| {
        AppError::internal(format!("reading the published export failed: {error}"))
    })?;
    let verified = project_store::checksum(&written) == outcome.checksum;
    let read = world_io::read_java_structure(&written).map_err(structure_error);
    let read = match (verified, read) {
        (true, Ok(read)) if read.structure == bundle.structure => read,
        _ => {
            let _ = std::fs::remove_file(&path);
            return Err(AppError::internal(
                "the published export failed binary or semantic verification",
            ));
        }
    };
    let size = read.inspection.size;
    let non_air_blocks = read.inspection.non_air_blocks;
    let data_version = read.inspection.data_version;
    Ok(StructureExportResult {
        outcome,
        verified: true,
        size,
        non_air_blocks,
        data_version,
        target_data_version: world_domain::TARGET_DATA_VERSION,
        target_minecraft_version: world_domain::TARGET_MINECRAFT_VERSION.to_owned(),
    })
}
