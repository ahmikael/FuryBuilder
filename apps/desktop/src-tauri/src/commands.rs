//! Typed Tauri IPC commands. Each command validates its own input and delegates to the
//! application facade, returning structured `AppError` values.

use asset_source::{
    AssetBundle, AssetBundleService, AssetDiscoveryReport, AssetPathReport, AssetSelectionKind,
    AssetSettingsStore, AssetSourceSetting, BlockStateRequest, SavedAssetSource,
};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use runtime_manager::{
    DatapackInspection, GenerationRequest, GenerationRunsReport, GenerationSnapshot,
    GenerationStatusReport, InstalledRuntime, InstalledServerJar, JavaRuntimeReport,
    ManagedCacheReport, ManagedServerCacheReport, RuntimeDiscoveryReport, RuntimeManager,
    RuntimeProgress, RuntimeReleaseInfo, SavedEula, ServerDiscoveryReport, ServerJarReport,
    ServerManager, ServerProgress, ServerReleaseInfo, TARGET_DATAPACK_FORMAT, TARGET_DATA_VERSION,
    TARGET_JAVA_MAJOR, TARGET_MINECRAFT_VERSION,
};
use tauri::{AppHandle, Emitter, State};
use world_application::{
    app_info as build_app_info, Actor, AppError, AppInfo, BlockChange, ChunkSnapshotBatch,
    ErrorCode, HistoryState, HistorySummary, Operation, OperationRequest, OperationResult,
    PingResponse, PreviewResult, Query, QueryResult, RegionBlocks, RenderSnapshot, WorldService,
};
use world_domain::{BlockBox, BlockDefinition, BlockPos, BlockState, ChunkPos, World, WorldInfo};
use world_io::{
    self, JavaStructureError, JavaWorldAnalysis, PaletteCount, Structure, StructureImportPreview,
};

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

fn world_import_error(error: world_io::WorldImportError) -> AppError {
    use world_application::ErrorCode;
    let code = match error.code() {
        "invalid_argument" | "unsafe_path" => ErrorCode::InvalidArgument,
        "not_found" => ErrorCode::NotFound,
        "conflict" | "source_changed" => ErrorCode::Conflict,
        "limit_exceeded" => ErrorCode::LimitExceeded,
        "unsupported_version" => ErrorCode::UnsupportedVersion,
        "unsupported_format" | "verification_failed" => ErrorCode::UnsupportedFormat,
        _ => ErrorCode::Internal,
    };
    AppError::new(code, error.to_string())
}

fn passthrough_error(error: world_io::PassthroughError) -> AppError {
    use world_application::ErrorCode;
    let code = match error.code() {
        "invalid_argument" | "unsafe_path" => ErrorCode::InvalidArgument,
        "not_found" => ErrorCode::NotFound,
        "already_exists" | "source_changed" => ErrorCode::Conflict,
        "limit_exceeded" => ErrorCode::LimitExceeded,
        "unsupported_version" => ErrorCode::UnsupportedVersion,
        "unsupported_format" | "verification_failed" => ErrorCode::UnsupportedFormat,
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
    state.jobs.cancel_all("the open project changed");
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
    state.jobs.cancel_all("the open project changed");
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
    state
        .jobs
        .cancel_all("the world was replaced by a checkpoint");
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
    state.jobs.cancel_all("the project was closed");
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
// Import World (M8.6). The selected world is analyzed and decoded read-only; the project
// receives a byte-preserving immutable source copy plus an editable block snapshot.
// ---------------------------------------------------------------------------------------

#[tauri::command]
pub fn world_import_preview(folder: String) -> JavaWorldAnalysis {
    world_io::analyze_java_world(&folder)
}

#[tauri::command]
pub fn project_import_world(
    state: State<'_, AppState>,
    name: String,
    world_folder: String,
    mut editor_state: project_store::EditorState,
) -> Result<OpenedProjectPayload, AppError> {
    let analysis = world_io::analyze_java_world(&world_folder);
    if !analysis.supported {
        let code = if analysis
            .blockers
            .iter()
            .any(|blocker| blocker.contains("DataVersion"))
        {
            ErrorCode::UnsupportedVersion
        } else {
            ErrorCode::UnsupportedFormat
        };
        return Err(AppError::new(
            code,
            format!(
                "the world cannot be imported: {}",
                analysis.blockers.join("; ")
            ),
        ));
    }
    let (mut world, _load_report) =
        world_io::load_java_world(&world_folder).map_err(world_import_error)?;
    world.bump_revision();
    if !world
        .dimensions()
        .contains_key(&editor_state.selected_dimension)
    {
        editor_state.selected_dimension = "minecraft:overworld".to_owned();
    }
    editor_state.validate().map_err(project_error)?;
    let info = world.info();
    let revision = world.revision();

    let mut projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    ensure_current_closed(&state, &mut projects)?;
    let detail = projects
        .store
        .create_imported(&name, editor_state.clone(), &world, "world")
        .map_err(project_error)?;
    let folder = detail.summary.folder.clone();
    let project_dir = projects.store.root().join(&folder);
    let source_world = project_dir.join(project_store::SOURCE_DIR).join("world");

    let built = (|| -> Result<project_store::ProjectDetail, AppError> {
        let copy = world_io::passthrough_java_world(&world_folder, &source_world)
            .map_err(passthrough_error)?;
        let files = copy
            .files
            .iter()
            .map(|file| {
                let relative = file.relative_path.to_string_lossy().replace('\\', "/");
                project_store::SourceFile {
                    relative_path: format!("world/{relative}"),
                    bytes: file.bytes,
                    sha256: file.sha256.clone(),
                }
            })
            .collect::<Vec<_>>();
        let detail = projects
            .store
            .record_source_tree(&folder, "world", &files)
            .map_err(project_error)?;
        projects
            .store
            .begin_session(&folder, revision)
            .map_err(project_error)?;
        Ok(detail)
    })();

    match built {
        Ok(detail) => {
            state.jobs.cancel_all("the open project changed");
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
            current_payload(&state, detail, info, None)
        }
        Err(error) => {
            drop(projects);
            // The project folder was created by this command and was never exposed as an
            // open project. Remove it so a failed import leaves no half-usable state.
            let _ = std::fs::remove_dir_all(&project_dir);
            Err(error)
        }
    }
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

#[tauri::command]
pub fn assets_discover() -> AssetDiscoveryReport {
    asset_source::discover_assets()
}

#[tauri::command]
pub fn assets_inspect_path(path: String) -> AssetPathReport {
    asset_source::inspect_asset_path(path)
}

#[tauri::command]
pub fn assets_saved_source(store: State<'_, AssetSettingsStore>) -> SavedAssetSource {
    store.saved_source()
}

#[tauri::command]
pub fn assets_save_source(
    app: AppHandle,
    store: State<'_, AssetSettingsStore>,
    kind: String,
    path: String,
) -> Result<SavedAssetSource, AppError> {
    let expected = match kind.as_str() {
        "minecraft_root" => AssetSelectionKind::MinecraftRoot,
        "client_jar" => AssetSelectionKind::ClientJar,
        "resource_pack" => AssetSelectionKind::ResourcePack,
        other => {
            return Err(AppError::invalid_argument(format!(
                "unknown asset source kind: {other}"
            )))
        }
    };
    let report = asset_source::inspect_asset_path(&path);
    let details = || serde_json::to_value(&report).unwrap_or(serde_json::Value::Null);
    if report.kind != expected {
        return Err(AppError::invalid_argument(format!(
            "the selected path is not a {}",
            kind.replace('_', " ")
        ))
        .with_details(details()));
    }
    if !report.valid {
        let message = report
            .errors
            .first()
            .cloned()
            .unwrap_or_else(|| "the selected asset source is not usable".to_owned());
        return Err(AppError::invalid_argument(message).with_details(details()));
    }
    if expected != AssetSelectionKind::ResourcePack && !report.target_match {
        return Err(AppError::invalid_argument(format!(
            "the selection does not provide a compatible Minecraft {} client",
            asset_source::TARGET_MINECRAFT_VERSION
        ))
        .with_details(details()));
    }
    store
        .save(AssetSourceSetting { kind, path })
        .map_err(asset_settings_error)
        .inspect(|_| {
            let _ = app.emit("furybuilder-assets-changed", ());
        })
}

#[tauri::command]
pub fn assets_clear_source(
    app: AppHandle,
    store: State<'_, AssetSettingsStore>,
) -> Result<SavedAssetSource, AppError> {
    store.clear().map_err(asset_settings_error).inspect(|_| {
        let _ = app.emit("furybuilder-assets-changed", ());
    })
}

#[tauri::command]
pub fn assets_save_pack_stack(
    app: AppHandle,
    store: State<'_, AssetSettingsStore>,
    paths: Vec<String>,
) -> Result<SavedAssetSource, AppError> {
    store
        .save_pack_stack(paths)
        .map_err(asset_settings_error)
        .inspect(|_| {
            let _ = app.emit("furybuilder-assets-changed", ());
        })
}

#[tauri::command]
pub fn assets_clear_pack_stack(
    app: AppHandle,
    store: State<'_, AssetSettingsStore>,
) -> Result<SavedAssetSource, AppError> {
    store
        .clear_pack_stack()
        .map_err(asset_settings_error)
        .inspect(|_| {
            let _ = app.emit("furybuilder-assets-changed", ());
        })
}

#[tauri::command]
pub fn assets_render_bundle(
    store: State<'_, AssetSettingsStore>,
    service: State<'_, AssetBundleService>,
    block_states: Vec<BlockStateRequest>,
    source_kind: Option<String>,
    source_path: Option<String>,
) -> Result<AssetBundle, AppError> {
    let source = match (source_kind, source_path) {
        (Some(kind), Some(path)) => Some(AssetSourceSetting { kind, path }),
        (None, None) => store.saved_source().source,
        _ => {
            return Err(AppError::invalid_argument(
                "sourceKind and sourcePath must be provided together",
            ))
        }
    };
    service
        .bundle(source.as_ref(), &store.pack_stack(), &block_states)
        .map_err(asset_bundle_error)
}

#[tauri::command]
pub fn assets_clear_cache(service: State<'_, AssetBundleService>) {
    service.clear_cache();
}

#[tauri::command]
pub async fn runtime_discover(
    manager: State<'_, RuntimeManager>,
) -> Result<RuntimeDiscoveryReport, AppError> {
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.discover())
        .await
        .map_err(|error| AppError::internal(format!("runtime discovery task failed: {error}")))
}

#[tauri::command]
pub fn runtime_saved(manager: State<'_, RuntimeManager>) -> Option<JavaRuntimeReport> {
    manager.saved()
}

#[tauri::command]
pub async fn runtime_select_java(
    app: AppHandle,
    manager: State<'_, RuntimeManager>,
    path: String,
) -> Result<JavaRuntimeReport, AppError> {
    let manager = manager.inner().clone();
    let report =
        tauri::async_runtime::spawn_blocking(move || manager.select(std::path::Path::new(&path)))
            .await
            .map_err(|error| AppError::internal(format!("runtime probe task failed: {error}")))?
            .map_err(runtime_error)?;
    let _ = app.emit("furybuilder-runtime-changed", ());
    Ok(report)
}

#[tauri::command]
pub fn runtime_clear_selection(
    app: AppHandle,
    manager: State<'_, RuntimeManager>,
) -> Result<(), AppError> {
    manager.clear_selection().map_err(runtime_error)?;
    let _ = app.emit("furybuilder-runtime-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn runtime_temurin_latest(
    manager: State<'_, RuntimeManager>,
) -> Result<RuntimeReleaseInfo, AppError> {
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.latest_temurin())
        .await
        .map_err(|error| AppError::internal(format!("Adoptium request task failed: {error}")))?
        .map_err(runtime_error)
}

#[tauri::command]
pub async fn runtime_install_temurin(
    app: AppHandle,
    manager: State<'_, RuntimeManager>,
    version: String,
) -> Result<InstalledRuntime, AppError> {
    let manager = manager.inner().clone();
    let progress_app = app.clone();
    let last = std::sync::Mutex::new(None::<(String, std::time::Instant)>);
    let installed = tauri::async_runtime::spawn_blocking(move || {
        manager.install_temurin(&version, &|progress: RuntimeProgress| {
            let should_emit = match last.lock() {
                Ok(previous) => match previous.as_ref() {
                    None => true,
                    Some((phase, at)) => {
                        *phase != progress.phase
                            || at.elapsed() >= std::time::Duration::from_millis(250)
                    }
                },
                Err(_) => true,
            };
            if should_emit {
                if let Ok(mut previous) = last.lock() {
                    *previous = Some((progress.phase.clone(), std::time::Instant::now()));
                }
                let _ = progress_app.emit("furybuilder-runtime-download", &progress);
            }
        })
    })
    .await
    .map_err(|error| AppError::internal(format!("managed Java install task failed: {error}")))?
    .map_err(runtime_error)?;
    let _ = app.emit("furybuilder-runtime-changed", ());
    Ok(installed)
}

#[tauri::command]
pub async fn runtime_managed_list(
    manager: State<'_, RuntimeManager>,
) -> Result<ManagedCacheReport, AppError> {
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.managed_list())
        .await
        .map_err(|error| AppError::internal(format!("runtime cache task failed: {error}")))?
        .map_err(runtime_error)
}

#[tauri::command]
pub async fn runtime_clear_managed(
    app: AppHandle,
    manager: State<'_, RuntimeManager>,
) -> Result<ManagedCacheReport, AppError> {
    let manager = manager.inner().clone();
    let report = tauri::async_runtime::spawn_blocking(move || manager.clear_managed())
        .await
        .map_err(|error| AppError::internal(format!("runtime cache task failed: {error}")))?
        .map_err(runtime_error)?;
    let _ = app.emit("furybuilder-runtime-changed", ());
    Ok(report)
}

#[tauri::command]
pub async fn server_discover(
    manager: State<'_, ServerManager>,
) -> Result<ServerDiscoveryReport, AppError> {
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.discover())
        .await
        .map_err(|error| AppError::internal(format!("server discovery task failed: {error}")))
}

#[tauri::command]
pub fn server_saved(manager: State<'_, ServerManager>) -> Option<ServerJarReport> {
    manager.selected()
}

#[tauri::command]
pub async fn server_select_jar(
    app: AppHandle,
    manager: State<'_, ServerManager>,
    path: String,
) -> Result<ServerJarReport, AppError> {
    let manager = manager.inner().clone();
    let report = tauri::async_runtime::spawn_blocking(move || {
        manager.select_manual(std::path::Path::new(&path))
    })
    .await
    .map_err(|error| AppError::internal(format!("server validation task failed: {error}")))?
    .map_err(server_error)?;
    let _ = app.emit("furybuilder-server-changed", ());
    Ok(report)
}

#[tauri::command]
pub fn server_clear_selection(
    app: AppHandle,
    manager: State<'_, ServerManager>,
) -> Result<(), AppError> {
    manager.clear_selection().map_err(server_error)?;
    let _ = app.emit("furybuilder-server-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn server_release(
    manager: State<'_, ServerManager>,
) -> Result<ServerReleaseInfo, AppError> {
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.release())
        .await
        .map_err(|error| AppError::internal(format!("Mojang metadata task failed: {error}")))?
        .map_err(server_error)
}

#[tauri::command]
pub async fn server_install(
    app: AppHandle,
    manager: State<'_, ServerManager>,
    version: String,
) -> Result<InstalledServerJar, AppError> {
    let manager = manager.inner().clone();
    let progress_app = app.clone();
    let last = std::sync::Mutex::new(None::<(String, std::time::Instant)>);
    let installed = tauri::async_runtime::spawn_blocking(move || {
        manager.install(&version, &|progress: ServerProgress| {
            let should_emit = match last.lock() {
                Ok(previous) => match previous.as_ref() {
                    None => true,
                    Some((phase, at)) => {
                        *phase != progress.phase
                            || at.elapsed() >= std::time::Duration::from_millis(250)
                    }
                },
                Err(_) => true,
            };
            if should_emit {
                if let Ok(mut previous) = last.lock() {
                    *previous = Some((progress.phase.clone(), std::time::Instant::now()));
                }
                let _ = progress_app.emit("furybuilder-server-download", &progress);
            }
        })
    })
    .await
    .map_err(|error| AppError::internal(format!("server JAR install task failed: {error}")))?;
    let installed = installed.map_err(server_error)?;
    let _ = app.emit("furybuilder-server-changed", ());
    Ok(installed)
}

#[tauri::command]
pub async fn server_managed_list(
    manager: State<'_, ServerManager>,
) -> Result<ManagedServerCacheReport, AppError> {
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.managed_list())
        .await
        .map_err(|error| AppError::internal(format!("server cache task failed: {error}")))?
        .map_err(server_error)
}

#[tauri::command]
pub async fn server_clear_managed(
    app: AppHandle,
    manager: State<'_, ServerManager>,
) -> Result<ManagedServerCacheReport, AppError> {
    let manager = manager.inner().clone();
    let report = tauri::async_runtime::spawn_blocking(move || manager.clear_managed())
        .await
        .map_err(|error| AppError::internal(format!("server cache task failed: {error}")))?
        .map_err(server_error)?;
    let _ = app.emit("furybuilder-server-changed", ());
    Ok(report)
}

#[tauri::command]
pub fn server_accept_eula(
    app: AppHandle,
    manager: State<'_, ServerManager>,
    accepted: bool,
) -> Result<SavedEula, AppError> {
    let eula = manager.accept_eula(accepted).map_err(server_error)?;
    let _ = app.emit("furybuilder-server-changed", ());
    Ok(eula)
}

#[tauri::command]
pub fn server_clear_eula(
    app: AppHandle,
    manager: State<'_, ServerManager>,
) -> Result<(), AppError> {
    manager.clear_eula().map_err(server_error)?;
    let _ = app.emit("furybuilder-server-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn server_start_generation(
    app: AppHandle,
    runtime: State<'_, RuntimeManager>,
    manager: State<'_, ServerManager>,
    request: GenerationRequest,
) -> Result<GenerationSnapshot, AppError> {
    let runtime = runtime.inner().clone();
    let java = tauri::async_runtime::spawn_blocking(move || runtime.saved())
        .await
        .map_err(|error| AppError::internal(format!("Java runtime task failed: {error}")))?;
    let Some(java) = java else {
        return Err(AppError::new(
            ErrorCode::InvalidArgument,
            "Select a compatible Java 21 runtime before starting generation.",
        ));
    };
    if !java.is_compatible() {
        return Err(AppError::new(
            ErrorCode::InvalidArgument,
            "The saved Java runtime is no longer compatible with Minecraft 1.21.4.",
        ));
    }
    let manager = manager.inner().clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        manager.start_generation(std::path::Path::new(&java.path), request)
    })
    .await
    .map_err(|error| AppError::internal(format!("server generation task failed: {error}")))?;
    let snapshot = snapshot.map_err(server_error)?;
    let _ = app.emit("furybuilder-server-changed", ());
    Ok(snapshot)
}

#[tauri::command]
pub fn server_generation_status(manager: State<'_, ServerManager>) -> GenerationStatusReport {
    manager.generation_status()
}

#[tauri::command]
pub fn server_cancel_generation(manager: State<'_, ServerManager>) -> bool {
    manager.cancel_generation()
}

#[tauri::command]
pub async fn server_generation_runs(
    manager: State<'_, ServerManager>,
) -> Result<GenerationRunsReport, AppError> {
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.generation_runs())
        .await
        .map_err(|error| AppError::internal(format!("generation runs task failed: {error}")))
}

#[tauri::command]
pub async fn server_clear_generation_runs(
    app: AppHandle,
    manager: State<'_, ServerManager>,
) -> Result<GenerationRunsReport, AppError> {
    let manager = manager.inner().clone();
    let report = tauri::async_runtime::spawn_blocking(move || manager.clear_generation_runs())
        .await
        .map_err(|error| AppError::internal(format!("generation runs task failed: {error}")))?;
    let report = report.map_err(server_error)?;
    let _ = app.emit("furybuilder-server-changed", ());
    Ok(report)
}

#[tauri::command]
pub async fn server_inspect_datapacks(
    manager: State<'_, ServerManager>,
    paths: Vec<String>,
) -> Result<Vec<DatapackInspection>, AppError> {
    if paths.len() > 16 {
        return Err(AppError::new(
            ErrorCode::LimitExceeded,
            "At most 16 data packs can be inspected at once.",
        ));
    }
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.inspect_datapacks(&paths))
        .await
        .map_err(|error| AppError::internal(format!("data pack inspection task failed: {error}")))
}

/// Combined Vanilla World setup check for the wizard. It reads the already verified runtime
/// and server selections; it never probes, downloads, accepts the EULA or starts anything.
#[tauri::command]
pub async fn vanilla_setup_check(
    runtime: State<'_, RuntimeManager>,
    manager: State<'_, ServerManager>,
) -> Result<VanillaSetupReport, AppError> {
    let runtime = runtime.inner().clone();
    let java = tauri::async_runtime::spawn_blocking(move || runtime.saved())
        .await
        .map_err(|error| AppError::internal(format!("Java runtime task failed: {error}")))?;
    let java_ready = java
        .as_ref()
        .map(JavaRuntimeReport::is_compatible)
        .unwrap_or(false);
    let discovery = manager.discover();
    let server_ready = discovery
        .selected
        .as_ref()
        .map(|server| server.ready)
        .unwrap_or(false);
    let eula_matches_selected = discovery.eula_matches_selected;
    let eula_accepted_at = discovery.eula.as_ref().map(|eula| eula.accepted_at);
    let mut blockers = Vec::new();
    match java.as_ref() {
        None => blockers.push(
            "Select a compatible Java 21 runtime (Settings → Java runtime or this wizard)."
                .to_owned(),
        ),
        Some(_) if !java_ready => blockers.push(
            "The saved Java runtime is not compatible with Minecraft Java 1.21.4.".to_owned(),
        ),
        Some(_) => {}
    }
    match discovery.selected.as_ref() {
        None => {
            blockers.push("Select or download the official Minecraft 1.21.4 server.jar.".to_owned())
        }
        Some(server) if !server.ready => blockers.push(format!(
            "The selected server JAR is not ready: {}",
            server
                .error
                .as_deref()
                .unwrap_or("it did not pass verification")
        )),
        Some(_) => {}
    }
    if !eula_matches_selected {
        blockers.push(
            "Read and accept the Minecraft EULA for the selected server.jar in this wizard."
                .to_owned(),
        );
    }
    let ready = java_ready && server_ready && eula_matches_selected;
    Ok(VanillaSetupReport {
        target_minecraft_version: TARGET_MINECRAFT_VERSION.to_owned(),
        target_data_version: TARGET_DATA_VERSION,
        target_java_major: TARGET_JAVA_MAJOR,
        server_datapack_format: TARGET_DATAPACK_FORMAT,
        java,
        java_ready,
        server: discovery.selected,
        server_ready,
        eula_accepted_at,
        eula_matches_selected,
        blockers,
        ready,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VanillaSetupReport {
    pub target_minecraft_version: String,
    pub target_data_version: i32,
    pub target_java_major: u32,
    pub server_datapack_format: u32,
    pub java: Option<JavaRuntimeReport>,
    pub java_ready: bool,
    pub server: Option<ServerJarReport>,
    pub server_ready: bool,
    pub eula_accepted_at: Option<u64>,
    pub eula_matches_selected: bool,
    pub blockers: Vec<String>,
    pub ready: bool,
}

fn server_error(error: runtime_manager::ServerError) -> AppError {
    let code = match error.code() {
        "invalid_argument" => ErrorCode::InvalidArgument,
        "unsupported_version" => ErrorCode::UnsupportedVersion,
        "unsupported_runtime" => ErrorCode::UnsupportedVersion,
        "eula_required" => ErrorCode::PermissionDenied,
        "consent_required" => ErrorCode::PermissionDenied,
        "network_error" => ErrorCode::BackendUnavailable,
        "checksum_mismatch" => ErrorCode::Conflict,
        "archive_error" | "unsupported_format" => ErrorCode::UnsupportedFormat,
        "limit_exceeded" => ErrorCode::LimitExceeded,
        "busy" => ErrorCode::Conflict,
        _ => ErrorCode::Internal,
    };
    AppError::new(code, error.to_string())
}

fn runtime_error(error: runtime_manager::RuntimeError) -> AppError {
    let code = match error.code() {
        "invalid_argument" => ErrorCode::InvalidArgument,
        "unsupported_version" => ErrorCode::UnsupportedVersion,
        "network_error" => ErrorCode::BackendUnavailable,
        "checksum_mismatch" => ErrorCode::Conflict,
        "archive_error" | "unsupported_format" => ErrorCode::UnsupportedFormat,
        "limit_exceeded" => ErrorCode::LimitExceeded,
        "busy" => ErrorCode::Conflict,
        _ => ErrorCode::Internal,
    };
    AppError::new(code, error.to_string())
}

fn asset_settings_error(error: asset_source::AssetSettingsError) -> AppError {
    let code = match error.code() {
        "invalid_argument" => ErrorCode::InvalidArgument,
        "unsupported_format" => ErrorCode::UnsupportedFormat,
        _ => ErrorCode::Internal,
    };
    AppError::new(code, error.to_string())
}

fn asset_bundle_error(error: asset_source::BundleError) -> AppError {
    let code = match error.code() {
        "invalid_argument" => ErrorCode::InvalidArgument,
        "limit_exceeded" => ErrorCode::LimitExceeded,
        "unsupported_format" => ErrorCode::UnsupportedFormat,
        _ => ErrorCode::Internal,
    };
    AppError::new(code, error.to_string())
}
