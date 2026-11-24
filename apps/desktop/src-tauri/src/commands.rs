//! Typed Tauri IPC commands. Each command validates its own input and delegates to the
//! application facade, returning structured `AppError` values.

use std::path::{Path, PathBuf};

use asset_source::{
    AssetBundle, AssetBundleService, AssetCatalogService, AssetDiscoveryReport, AssetPathReport,
    AssetSelectionKind, AssetSettingsStore, AssetSourceSetting, BlockStateRequest, CatalogSnapshot,
    SavedAssetSource,
};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use component_manager::{
    CancelOutcome, ClientJarReleaseInfo, ComponentError, ComponentInstallRequest,
    ComponentInventory, ComponentKind, ComponentManager, ComponentProgress, ComponentRemoval,
    ComponentReport, ComponentRequirements, ComponentStatus, ComponentTarget,
    ComponentVerification, ResolvedComponent,
};
use minecraft_version::{
    builtin_profiles, default_profile, ManifestVersionType, VersionCatalog, VersionProfile,
};
use runtime_manager::{
    DatapackInspection, GeneratedWorld, GenerationEstimate, GenerationLimits, GenerationRequest,
    GenerationRunsReport, GenerationSnapshot, GenerationStatusReport, InstalledRuntime,
    InstalledServerJar, JavaRuntimeReport, ManagedCacheReport, ManagedServerCacheReport,
    RuntimeDiscoveryReport, RuntimeProgress, RuntimeReleaseInfo, SavedEula, ServerDiscoveryReport,
    ServerJarReport, ServerProgress, ServerReleaseInfo,
};
use structure_library::{LibraryError, StructureLibrary, TemplateEntry, TemplateSource};
use tauri::{AppHandle, Emitter, State};
use world_application::{
    app_info as build_app_info, compile_build_dsl, Actor, AppError, AppInfo, BlockChange,
    BuildDslSummary, BuildRecipe, ChunkSnapshotBatch, ErrorCode, HistoryState, HistorySummary,
    JobSnapshot, JobStartRequest, ObjectKind, Observation, ObservationRequest, Operation,
    OperationRequest, OperationResult, ParametricApplyResult, ParametricConflictPolicy,
    ParametricInspection, ParametricObjectSummary, ParametricScene, ParametricSpec, PingResponse,
    PreviewResult, Query, QueryResult, RegionBlocks, RenderSnapshot, SceneObjectInspection,
    SemanticObject, StructureSites, ValidationQuery, ValidationReport, WorkspacePolicy,
    WorldService,
};
use world_domain::{
    BiomeState, BlockBox, BlockDefinition, BlockPos, BlockState, ChunkPos, World, WorldInfo,
};
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

/// Renders a deterministic top-down, height, material, biome, cross-section or revision
/// diff observation. It is read-only and returns a PNG data URL with bounded metadata.
#[tauri::command]
pub fn observe(
    state: State<'_, AppState>,
    request: ObservationRequest,
) -> Result<Observation, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    service.observe(&request)
}

/// Runs bounded, read-only build validators and returns structured findings plus suggested
/// repair operations. Nothing is written; repairs use the normal preview/apply path.
#[tauri::command]
pub fn validate_region(
    state: State<'_, AppState>,
    request: ValidationQuery,
) -> Result<ValidationReport, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    service.validate_region(&request)
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

/// A typed Build DSL compilation plus its ordinary operation preview. The returned
/// operation is passed to the same apply command as any other batch.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildDslPreview {
    pub summary: BuildDslSummary,
    pub operation: Operation,
    pub preview: PreviewResult,
}

#[tauri::command]
pub fn build_dsl_preview(
    state: State<'_, AppState>,
    recipe: BuildRecipe,
) -> Result<BuildDslPreview, AppError> {
    build_dsl_preview_impl(&state, recipe)
}

fn build_dsl_preview_impl(
    state: &AppState,
    recipe: BuildRecipe,
) -> Result<BuildDslPreview, AppError> {
    let compiled = compile_build_dsl(&recipe)?;
    let operation = compiled.operation();
    let preview = {
        let service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        service.preview(LOCAL_PROJECT_ID, &operation)?
    };
    Ok(BuildDslPreview {
        summary: compiled.summary().clone(),
        operation,
        preview,
    })
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
        layer_id: None,
    })
}

/// Starts a cancellable UI job through the same bounded queue used by MCP. The editor
/// uses it for large pastes so progress and cancellation come from the Rust operation
/// observer instead of a simulated client-side timer.
#[tauri::command]
pub fn start_world_job(
    state: State<'_, AppState>,
    kind: String,
    parameters: serde_json::Value,
    expected_revision: Option<u64>,
    idempotency_key: String,
) -> Result<JobSnapshot, AppError> {
    state.jobs.start(JobStartRequest {
        project_id: LOCAL_PROJECT_ID.to_owned(),
        kind,
        parameters,
        expected_revision,
        idempotency_key,
        actor: ui_actor(),
    })
}

#[tauri::command]
pub fn world_job_status(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<JobSnapshot, AppError> {
    state.jobs.status(LOCAL_PROJECT_ID, &job_id)
}

#[tauri::command]
pub fn cancel_world_job(
    state: State<'_, AppState>,
    job_id: String,
) -> Result<JobSnapshot, AppError> {
    state.jobs.cancel(LOCAL_PROJECT_ID, &job_id)
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

#[tauri::command]
pub fn get_biome(
    state: State<'_, AppState>,
    dimension: String,
    pos: BlockPos,
) -> Result<Option<BiomeState>, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    Ok(service.world().get_biome(&dimension, pos)?)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn find_structure_sites(
    state: State<'_, AppState>,
    dimension: String,
    bounds: BlockBox,
    width: u32,
    depth: u32,
    step: u32,
    max_slope: u32,
    clearance: u32,
    limit: u32,
) -> Result<StructureSites, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    match service.query(Query::FindStructureSites {
        dimension,
        bounds,
        width,
        depth,
        step,
        max_slope,
        clearance,
        limit,
    })? {
        QueryResult::StructureSites(result) => Ok(result),
        _ => Err(AppError::internal(
            "unexpected query result for structure sites",
        )),
    }
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

fn world_source_error(error: world_io::WorldSourceError) -> AppError {
    use world_application::ErrorCode;
    let code = match error.code() {
        "invalid_argument" | "unsafe_archive_entry" => ErrorCode::InvalidArgument,
        "not_found" => ErrorCode::NotFound,
        "limit_exceeded" => ErrorCode::LimitExceeded,
        "unsupported_version" => ErrorCode::UnsupportedVersion,
        "invalid_archive" | "unsupported_format" => ErrorCode::UnsupportedFormat,
        _ => ErrorCode::Internal,
    };
    AppError::new(code, error.to_string())
}

fn parity_error(error: world_io::ParityError) -> AppError {
    use world_application::ErrorCode;
    let code = match error.code() {
        "invalid_argument" | "unsafe_path" => ErrorCode::InvalidArgument,
        "not_found" => ErrorCode::NotFound,
        "limit_exceeded" => ErrorCode::LimitExceeded,
        "unsupported_version" => ErrorCode::UnsupportedVersion,
        "unsupported_format" => ErrorCode::UnsupportedFormat,
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

/// The library location shown by the creation wizard. The backend is the only owner of
/// the default projects folder and the project folder suffix.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDefaults {
    pub projects_root: String,
    pub folder_suffix: String,
    pub flat: project_store::FlatDefaults,
}

/// One official version entry plus the verified profile this build actually supports.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionCatalogEntryReport {
    pub id: String,
    pub version_type: ManifestVersionType,
    pub release_time: String,
    pub latest_release: bool,
    pub latest_snapshot: bool,
    pub recommended: bool,
    pub profile: Option<VersionProfile>,
    pub unavailable_reason: Option<String>,
}

/// The official version catalog shaped for the creation wizard. No JAR bytes are involved.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionCatalogReport {
    pub source: String,
    pub fetched_at_unix: u64,
    pub latest_release: String,
    pub latest_snapshot: String,
    pub recommended: String,
    pub versions: Vec<VersionCatalogEntryReport>,
}

fn build_version_catalog_report(catalog: &VersionCatalog) -> VersionCatalogReport {
    let manifest = catalog.manifest();
    let profiles = builtin_profiles();
    let recommended = default_profile().minecraft_version.to_string();
    let latest_release = manifest.latest.release.to_string();
    let latest_snapshot = manifest.latest.snapshot.to_string();

    let versions = manifest
        .versions
        .iter()
        .filter(|entry| {
            matches!(
                entry.version_type,
                ManifestVersionType::Release | ManifestVersionType::Snapshot
            )
        })
        .map(|entry| {
            let profile = profiles
                .iter()
                .find(|profile| profile.minecraft_version == entry.id)
                .cloned();
            let version_id = entry.id.to_string();
            let unavailable_reason = profile.is_none().then(|| {
                if entry.version_type == ManifestVersionType::Snapshot {
                    "This build has no verified snapshot profile; snapshots are read-only or convertible."
                        .to_owned()
                } else {
                    "This build has no verified profile for this release yet.".to_owned()
                }
            });
            VersionCatalogEntryReport {
                latest_release: entry.id == manifest.latest.release,
                latest_snapshot: entry.id == manifest.latest.snapshot,
                recommended: version_id == recommended,
                id: version_id,
                version_type: entry.version_type,
                release_time: entry.release_time.clone(),
                profile,
                unavailable_reason,
            }
        })
        .collect();

    VersionCatalogReport {
        source: match catalog.source() {
            minecraft_version::CatalogSource::Network => "network".to_owned(),
            minecraft_version::CatalogSource::Cache => "cache".to_owned(),
        },
        fetched_at_unix: catalog.fetched_at_unix(),
        latest_release,
        latest_snapshot,
        recommended,
        versions,
    }
}

#[tauri::command]
pub async fn versions_catalog(
    components: State<'_, ComponentManager>,
) -> Result<VersionCatalogReport, AppError> {
    let components = components.inner().clone();
    tauri::async_runtime::spawn_blocking(move || components.version_catalog())
        .await
        .map_err(|error| AppError::internal(format!("version catalog task failed: {error}")))?
        .map(|catalog| build_version_catalog_report(&catalog))
        .map_err(component_error)
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

/// Replaces the authoritative service for a newly opened world and installs the
/// project's persisted workspace policy and semantic scene before any command can run.
/// Doing this in one place prevents a project from briefly opening with default metadata.
fn install_world_service(
    state: &AppState,
    world: World,
    editor_state: &project_store::EditorState,
) -> Result<(), AppError> {
    let mut replacement = WorldService::from_world(world);
    replacement.set_workspace_policy(editor_state.workspace.clone())?;
    replacement.set_scene(editor_state.scene.clone())?;
    replacement.set_parametric_scene(editor_state.parametric.clone())?;
    let mut service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    *service = replacement;
    Ok(())
}

fn current_project(state: &AppState) -> Result<CurrentProject, AppError> {
    state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?
        .current
        .clone()
        .ok_or_else(|| AppError::new(ErrorCode::NotFound, "no project is open"))
}

#[tauri::command]
pub fn project_status(state: State<'_, AppState>) -> Result<SessionStatus, AppError> {
    session_status(&state)
}

#[tauri::command]
pub fn diagnostics_info(state: State<'_, AppState>) -> crate::diagnostics::DiagnosticsInfo {
    state.diagnostics.info()
}

#[tauri::command]
pub fn diagnostics_clear(
    state: State<'_, AppState>,
) -> Result<crate::diagnostics::DiagnosticsInfo, AppError> {
    state.diagnostics.clear().map_err(AppError::internal)
}

#[tauri::command]
pub fn diagnostics_open_folder(state: State<'_, AppState>) -> Result<(), AppError> {
    state.diagnostics.open_folder().map_err(AppError::internal)
}

/// Accepts a sanitized UI-level crash/error report. It never contains world payloads;
/// absolute paths and control characters are removed by the diagnostics sink.
#[tauri::command]
pub fn diagnostics_record_ui_error(state: State<'_, AppState>, message: String, source: String) {
    state.diagnostics.write(
        "error",
        "ui.error",
        serde_json::json!({ "message": message, "source": source }),
    );
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
    folder: Option<String>,
    profile_id: Option<String>,
    flat: Option<project_store::FlatPreset>,
) -> Result<OpenedProjectPayload, AppError> {
    let mut projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    ensure_current_closed(&state, &mut projects)?;
    let detail = projects
        .store
        .create_with_options(
            &name,
            mode,
            editor_state.clone(),
            project_store::CreateOptions {
                folder: folder.as_deref(),
                profile_id: profile_id.as_deref(),
                flat,
            },
        )
        .map_err(project_error)?;
    let folder = detail.summary.folder.clone();
    let (detail, world, recovery) = projects.store.open(&folder).map_err(project_error)?;
    projects
        .store
        .begin_session(&folder, world.revision())
        .map_err(project_error)?;
    let info = world.info();
    state.jobs.cancel_all("the open project changed");
    install_world_service(&state, world, &editor_state)?;
    projects.current = Some(CurrentProject {
        folder: folder.clone(),
        project_id: detail.summary.project_id.clone(),
        name: detail.summary.name.clone(),
        target: detail.target.clone(),
        saved_revision: detail.summary.world_revision,
        autosaved_revision: None,
        editor_state,
    });
    projects.refresh_checkpoints(&folder);
    drop(projects);
    state.diagnostics.write(
        "info",
        "project.create",
        serde_json::json!({
            "folder": folder,
            "revision": detail.summary.world_revision,
            "mode": format!("{:?}", detail.summary.creation_mode).to_lowercase(),
        }),
    );
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
    install_world_service(&state, world, &detail.editor_state)?;
    projects.current = Some(CurrentProject {
        folder: folder.clone(),
        project_id: detail.summary.project_id.clone(),
        name: detail.summary.name.clone(),
        target: detail.target.clone(),
        saved_revision: detail.summary.world_revision,
        autosaved_revision: recovery.as_ref().map(|info| info.recovered_revision),
        editor_state: detail.editor_state.clone(),
    });
    projects.refresh_checkpoints(&folder);
    drop(projects);
    state.diagnostics.write(
        "info",
        "project.open",
        serde_json::json!({
            "folder": folder,
            "revision": info.revision,
            "recoveryAvailable": recovery.as_ref().map(|entry| entry.available),
            "recoveryReason": recovery.as_ref().map(|entry| entry.reason.clone()),
        }),
    );
    current_payload(&state, detail, info, recovery)
}

#[tauri::command]
pub fn project_save(
    state: State<'_, AppState>,
    mut editor_state: project_store::EditorState,
) -> Result<project_store::SaveOutcome, AppError> {
    let mut projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    let current = projects.current.clone().ok_or_else(|| {
        AppError::new(world_application::ErrorCode::NotFound, "no project is open")
    })?;
    // Baselines are large and live only in Rust; never trust or require the UI to echo them.
    editor_state.parametric = current.editor_state.parametric.clone();
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
    state.diagnostics.write(
        "info",
        "project.save",
        serde_json::json!({
            "folder": current.folder,
            "revision": outcome.project.summary.world_revision,
            "bytes": outcome.bytes_written,
            "backupCreated": outcome.backup_created,
            "prunedSnapshots": outcome.pruned_snapshots,
        }),
    );
    Ok(outcome)
}

#[tauri::command]
pub fn project_autosave(
    state: State<'_, AppState>,
    mut editor_state: project_store::EditorState,
    reason: Option<String>,
) -> Result<u64, AppError> {
    let mut projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    let current = projects.current.clone().ok_or_else(|| {
        AppError::new(world_application::ErrorCode::NotFound, "no project is open")
    })?;
    editor_state.parametric = current.editor_state.parametric.clone();
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
    state.diagnostics.write(
        "debug",
        "project.autosave",
        serde_json::json!({
            "folder": current.folder,
            "revision": revision,
            "reason": reason.unwrap_or_default(),
        }),
    );
    Ok(revision)
}

#[tauri::command]
pub fn project_checkpoint(
    state: State<'_, AppState>,
    reason: String,
    label: Option<String>,
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
            label.as_deref(),
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
    let opened = projects
        .store
        .open_checkpoint(&folder, revision)
        .map_err(project_error)?;
    let info = opened.world.info();
    state
        .jobs
        .cancel_all("the world was replaced by a checkpoint");
    install_world_service(&state, opened.world, &opened.meta.editor_state)?;
    if let Some(current) = projects.current.as_mut() {
        if current.folder == folder {
            current.editor_state = opened.meta.editor_state;
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
    mut editor_state: project_store::EditorState,
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
        editor_state.parametric = current.editor_state.parametric.clone();
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

/// Temporary extraction directory for a `.zip` world archive. It is removed when the
/// command that owns it finishes, whether the analysis/import succeeded or failed.
struct ImportStaging {
    path: PathBuf,
}

impl ImportStaging {
    fn new() -> Self {
        Self {
            path: std::env::temp_dir().join(format!("furybuilder-import-{}", uuid::Uuid::new_v4())),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ImportStaging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[tauri::command]
pub fn world_import_preview(source: String) -> JavaWorldAnalysis {
    let staging = ImportStaging::new();
    world_io::analyze_world_source(&source, staging.path())
}

#[tauri::command]
pub fn project_defaults(state: State<'_, AppState>) -> Result<ProjectDefaults, AppError> {
    let projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    Ok(ProjectDefaults {
        projects_root: projects.store.root().display().to_string(),
        folder_suffix: format!(".{}", project_store::PROJECT_EXTENSION),
        flat: project_store::FlatDefaults::default(),
    })
}

#[tauri::command]
pub fn project_import_world(
    state: State<'_, AppState>,
    name: String,
    source: String,
    strategy: world_io::ImportStrategy,
    editor_state: project_store::EditorState,
    folder: Option<String>,
) -> Result<OpenedProjectPayload, AppError> {
    if strategy != world_io::ImportStrategy::ImportAsIs {
        return Err(AppError::new(
            ErrorCode::UnsupportedVersion,
            "conversion is not available for this source; choose Import as-is",
        ));
    }
    let staging = ImportStaging::new();
    let prepared =
        world_io::prepare_world_source(&source, staging.path()).map_err(world_source_error)?;
    import_world_project(
        state.inner(),
        name,
        prepared.root(),
        editor_state,
        None,
        folder.as_deref(),
    )
}

/// Imports a completed official-server generation run. The run is re-verified (manifest,
/// completed report, world directory) before reading; the generated world becomes an
/// immutable source copy and its exact generator identity is stored in `project.json`.
#[tauri::command]
pub fn project_import_generated_world(
    state: State<'_, AppState>,
    components: State<'_, ComponentManager>,
    name: String,
    run_id: String,
    editor_state: project_store::EditorState,
    folder: Option<String>,
) -> Result<OpenedProjectPayload, AppError> {
    let generated = components
        .server()
        .generated_world(&run_id)
        .map_err(server_error)?;
    let generated_version = world_io::analyze_java_world(&generated.world_dir);
    let minecraft_version = generated_version
        .version_name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| {
            AppError::new(
                ErrorCode::UnsupportedVersion,
                "the generated world does not declare a Minecraft version",
            )
        })?;
    let data_version = generated_version.data_version.ok_or_else(|| {
        AppError::new(
            ErrorCode::UnsupportedVersion,
            "the generated world does not declare a DataVersion",
        )
    })?;
    let provenance = project_store::VanillaProvenance {
        seed: generated.seed.clone(),
        preset: generated.preset.label().to_owned(),
        center_x: generated.center_x,
        center_z: generated.center_z,
        radius_chunks: generated.radius_chunks,
        generate_structures: generated.generate_structures,
        datapacks: generated
            .datapacks
            .iter()
            .map(|datapack| project_store::VanillaDatapack {
                name: datapack.name.clone(),
                sha256: datapack.sha256.clone(),
                size: datapack.size,
                pack_format: datapack.pack_format,
            })
            .collect(),
        minecraft_version,
        data_version,
        java_version: generated.java_version,
        server_jar_sha1: generated.server_sha1,
        run_id: generated.run_id,
        generated_at_ms: generated.finished_at,
        chunks: generated.chunks,
        region_files: generated.region_files,
        world_bytes: generated.world_bytes,
    };
    let name = if name.trim().is_empty() {
        format!("Vanilla {}", generated.seed)
    } else {
        name
    };
    import_world_project(
        state.inner(),
        name,
        Path::new(&generated.world_dir),
        editor_state,
        Some(provenance),
        folder.as_deref(),
    )
}

fn import_world_project(
    state: &AppState,
    name: String,
    world_root: &Path,
    mut editor_state: project_store::EditorState,
    vanilla: Option<project_store::VanillaProvenance>,
    folder: Option<&str>,
) -> Result<OpenedProjectPayload, AppError> {
    let analysis = world_io::analyze_java_world(world_root);
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
        world_io::load_java_world(world_root).map_err(world_import_error)?;
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
    ensure_current_closed(state, &mut projects)?;
    let detail = match vanilla {
        Some(provenance) => projects.store.create_vanilla(
            &name,
            editor_state.clone(),
            &world,
            "world",
            provenance,
            folder,
        ),
        None => {
            projects
                .store
                .create_imported(&name, editor_state.clone(), &world, "world", folder)
        }
    }
    .map_err(project_error)?;
    let folder = detail.summary.folder.clone();
    let project_dir = projects.store.root().join(&folder);
    let source_world = project_dir.join(project_store::SOURCE_DIR).join("world");

    let built = (|| -> Result<project_store::ProjectDetail, AppError> {
        let copy = world_io::passthrough_java_world(world_root, &source_world)
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
            install_world_service(state, world, &editor_state)?;
            projects.current = Some(CurrentProject {
                folder: folder.clone(),
                project_id: detail.summary.project_id.clone(),
                name: detail.summary.name.clone(),
                target: detail.target.clone(),
                saved_revision: detail.summary.world_revision,
                autosaved_revision: None,
                editor_state,
            });
            projects.refresh_checkpoints(&folder);
            drop(projects);
            current_payload(state, detail, info, None)
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
// Generated-base parity (M9.8). The reference is the original generator output; the
// candidate is the saved editable snapshot. Settings and provenance must agree exactly
// before any chunk is compared, and an unsupported reference configuration is refused.
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VanillaParityReport {
    /// `matched`, `mismatched` or `refused`.
    pub status: String,
    pub project_folder: String,
    pub project_name: String,
    pub snapshot_revision: u64,
    pub from_autosave: bool,
    /// `generatedRun` or `projectSource`.
    pub reference_kind: String,
    pub reference_root: String,
    pub seed: String,
    pub preset: String,
    pub center_x: i32,
    pub center_z: i32,
    pub radius_chunks: u32,
    pub refusals: Vec<String>,
    pub comparison: Option<world_io::ParityReport>,
    pub limitations: Vec<String>,
}

fn reference_settings_mismatches(
    provenance: &project_store::VanillaProvenance,
    generated: &GeneratedWorld,
) -> Vec<String> {
    let mut mismatches = Vec::new();
    if provenance.seed != generated.seed {
        mismatches.push(format!(
            "seed: project `{}` vs reference `{}`",
            provenance.seed, generated.seed
        ));
    }
    if provenance.preset != generated.preset.label() {
        mismatches.push(format!(
            "preset: project `{}` vs reference `{}`",
            provenance.preset,
            generated.preset.label()
        ));
    }
    if provenance.center_x != generated.center_x {
        mismatches.push(format!(
            "center x: project {} vs reference {}",
            provenance.center_x, generated.center_x
        ));
    }
    if provenance.center_z != generated.center_z {
        mismatches.push(format!(
            "center z: project {} vs reference {}",
            provenance.center_z, generated.center_z
        ));
    }
    if provenance.radius_chunks != generated.radius_chunks {
        mismatches.push(format!(
            "radius: project {} vs reference {} chunk(s)",
            provenance.radius_chunks, generated.radius_chunks
        ));
    }
    if provenance.generate_structures != generated.generate_structures {
        mismatches.push(format!(
            "generate-structures: project {} vs reference {}",
            provenance.generate_structures, generated.generate_structures
        ));
    }
    if provenance.datapacks.len() != generated.datapacks.len() {
        mismatches.push(format!(
            "data packs: project has {} vs reference has {}",
            provenance.datapacks.len(),
            generated.datapacks.len()
        ));
    } else {
        for (index, (expected, actual)) in provenance
            .datapacks
            .iter()
            .zip(generated.datapacks.iter())
            .enumerate()
        {
            if expected.name != actual.name
                || expected.sha256 != actual.sha256
                || expected.size != actual.size
                || expected.pack_format != actual.pack_format
            {
                mismatches.push(format!(
                    "data pack #{}: project `{}` ({} bytes) vs reference `{}` ({} bytes)",
                    index + 1,
                    expected.name,
                    expected.size,
                    actual.name,
                    actual.size
                ));
            }
        }
    }
    if provenance.java_version != generated.java_version {
        mismatches.push(format!(
            "Java version: project `{}` vs reference `{}`",
            provenance.java_version, generated.java_version
        ));
    }
    if provenance.server_jar_sha1 != generated.server_sha1 {
        mismatches.push(format!(
            "official server JAR SHA-1: project `{}` vs reference `{}`",
            provenance.server_jar_sha1, generated.server_sha1
        ));
    }
    if provenance.chunks != generated.chunks {
        mismatches.push(format!(
            "generated chunks: project {} vs reference {}",
            provenance.chunks, generated.chunks
        ));
    }
    if provenance.region_files != generated.region_files {
        mismatches.push(format!(
            "region files: project {} vs reference {}",
            provenance.region_files, generated.region_files
        ));
    }
    if provenance.world_bytes != generated.world_bytes {
        mismatches.push(format!(
            "generated world bytes: project {} vs reference {}",
            provenance.world_bytes, generated.world_bytes
        ));
    }
    mismatches
}

fn parity_refused(mut report: VanillaParityReport, reasons: Vec<String>) -> VanillaParityReport {
    report.status = "refused".to_owned();
    report.refusals = reasons;
    report
}

/// Compares a saved Vanilla World base with the original generator's reference chunks.
///
/// The project must still be the untouched imported base (revision 1). The preferred
/// reference is the completed generation run recorded in `project.json`; when that run has
/// been cleared, the project's immutable byte-for-byte source copy is used and the report
/// says so. Settings/provenance must agree exactly, and unsupported layouts or incomplete
/// reference chunks are refused instead of producing a false match.
#[tauri::command]
pub fn project_verify_vanilla_parity(
    state: State<'_, AppState>,
    components: State<'_, ComponentManager>,
    folder: String,
    run_id: Option<String>,
) -> Result<VanillaParityReport, AppError> {
    let (detail, world, recovery, project_dir) = {
        let projects = state
            .projects
            .lock()
            .map_err(|_| AppError::internal("project state lock poisoned"))?;
        let (detail, world, recovery) = projects.store.open(&folder).map_err(project_error)?;
        let project_dir = projects.store.root().join(&folder);
        (detail, world, recovery, project_dir)
    };
    let provenance = detail.vanilla.clone().ok_or_else(|| {
        AppError::new(
            ErrorCode::InvalidArgument,
            "reference-chunk parity requires an imported Vanilla World project",
        )
    })?;
    let mut report = VanillaParityReport {
        status: "refused".to_owned(),
        project_folder: folder.clone(),
        project_name: detail.summary.name.clone(),
        snapshot_revision: world.revision(),
        from_autosave: recovery.is_some(),
        reference_kind: "generatedRun".to_owned(),
        reference_root: String::new(),
        seed: provenance.seed.clone(),
        preset: provenance.preset.clone(),
        center_x: provenance.center_x,
        center_z: provenance.center_z,
        radius_chunks: provenance.radius_chunks,
        refusals: Vec::new(),
        comparison: None,
        limitations: Vec::new(),
    };

    if detail.summary.creation_mode != project_store::CreationMode::Vanilla
        || detail.source.kind != project_store::SourceKind::Generated
        || !detail.source.immutable
    {
        return Ok(parity_refused(
            report,
            vec![
                "the project source is not an immutable generated base; parity is only defined for Vanilla World projects"
                    .to_owned(),
            ],
        ));
    }
    if world.revision() != 1 {
        return Ok(parity_refused(
            report,
            vec![format!(
                "the saved snapshot is revision {}; reference-chunk parity is only defined for the untouched imported base (revision 1). The immutable source copy is still preserved",
                world.revision()
            )],
        ));
    }
    if report.from_autosave {
        report.limitations.push(format!(
            "the verified autosave at revision {} was checked instead of the last manual save",
            world.revision()
        ));
    }

    let source_reference = detail
        .source
        .reference
        .clone()
        .unwrap_or_else(|| "world".to_owned());
    let source_world = project_dir
        .join(project_store::SOURCE_DIR)
        .join(&source_reference);
    let explicit_run = run_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let preferred_run = explicit_run
        .clone()
        .unwrap_or_else(|| provenance.run_id.clone());
    let reference_root = match components.server().generated_world(&preferred_run) {
        Ok(generated) => {
            report.reference_root = generated.world_dir.clone();
            let mismatches = reference_settings_mismatches(&provenance, &generated);
            if !mismatches.is_empty() {
                return Ok(parity_refused(report, mismatches));
            }
            generated.world_dir
        }
        Err(error) => {
            if explicit_run.is_some() {
                return Ok(parity_refused(
                    report,
                    vec![format!(
                        "the requested reference run `{preferred_run}` did not pass verification: {error}"
                    )],
                ));
            }
            report.reference_kind = "projectSource".to_owned();
            report.reference_root = source_world.display().to_string();
            report.limitations.push(format!(
                "the original run `{preferred_run}` could not be reopened ({error}); the project's immutable source copy was checked instead"
            ));
            source_world.display().to_string()
        }
    };
    report.reference_root = reference_root.clone();
    if !std::path::Path::new(&reference_root).is_dir() {
        return Ok(parity_refused(
            report,
            vec![format!(
                "the reference world `{reference_root}` is missing or is not a directory"
            )],
        ));
    }

    // Analysis is read-only and returns blockers for unsupported versions/layouts or a
    // malformed reference instead of letting the comparison make an unwarranted claim.
    let analysis = world_io::analyze_java_world(&reference_root);
    if !analysis.supported {
        let reasons = if analysis.blockers.is_empty() {
            vec![format!(
                "the reference world is not a supported complete {} save",
                runtime_manager::target_minecraft_version()
            )]
        } else {
            analysis.blockers
        };
        return Ok(parity_refused(report, reasons));
    }

    let chunks = world_io::reference_chunks(
        provenance.center_x,
        provenance.center_z,
        provenance.radius_chunks,
        world_io::MAX_PARITY_CHUNKS,
    );
    let comparison = world_io::compare_java_world_to_world(
        &reference_root,
        &chunks,
        &world,
        world_io::ParityOptions::default(),
    )
    .map_err(parity_error)?;
    report.status = match comparison.status {
        world_io::ParityStatus::Matched => "matched",
        world_io::ParityStatus::Mismatched => "mismatched",
        world_io::ParityStatus::Refused => "refused",
    }
    .to_owned();
    report.refusals = comparison.refusals.clone();
    for limitation in &comparison.limitations {
        if !report.limitations.contains(limitation) {
            report.limitations.push(limitation.clone());
        }
    }
    report.comparison = Some(comparison);
    Ok(report)
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

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInsertPreview {
    pub entry: TemplateEntry,
    pub preview: StructureImportPreview,
    pub bounds: BlockBox,
    pub affected_blocks: u64,
    pub changed_blocks: u64,
    pub warnings: Vec<String>,
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
    // `CON.nbt`, `NUL.nbt` and similar names address Windows devices even with an
    // extension. Fall back to a neutral name instead of failing the import.
    if !project_store::is_safe_file_name(&cleaned) {
        cleaned = "structure.nbt".to_owned();
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

pub(crate) fn template_error(error: LibraryError) -> AppError {
    let code = match error.code() {
        "not_found" => ErrorCode::NotFound,
        "limit_exceeded" => ErrorCode::LimitExceeded,
        "invalid_argument" => ErrorCode::InvalidArgument,
        _ => ErrorCode::Internal,
    };
    AppError::new(code, error.to_string())
}

pub(crate) fn current_template_library(state: &AppState) -> Result<StructureLibrary, AppError> {
    let projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    let current = projects.current.as_ref().ok_or_else(|| {
        AppError::new(
            ErrorCode::NotFound,
            "no project is open; open a project before using templates",
        )
    })?;
    let directory = projects
        .store
        .templates_dir(&current.folder)
        .map_err(project_error)?;
    Ok(StructureLibrary::new(directory))
}

/// Loads and re-verifies a template, then turns it into an ordinary bounded Batch
/// operation. The payload is never trusted just because it was indexed earlier.
pub(crate) fn prepare_template_insert(
    state: &AppState,
    template_id: &str,
    dimension: &str,
    origin: [i32; 3],
    include_air: bool,
) -> Result<(TemplateEntry, StructureImportPreview, BlockBox, Operation), AppError> {
    let library = current_template_library(state)?;
    let (bytes, entry) = library.load_bytes(template_id).map_err(template_error)?;
    let parsed = world_io::read_verified_import(&bytes).map_err(structure_error)?;
    let preview = world_io::preview_from_structure(&parsed.structure, &parsed.inspection);
    if !preview.compatible {
        return Err(AppError::new(
            ErrorCode::UnsupportedFormat,
            preview.warnings.join("; "),
        ));
    }
    let origin = placement_origin(dimension, origin, preview.size)?;
    let changes = import_changes(&parsed.structure, origin, include_air)?;
    let bounds = operation_bounds(origin, preview.size)?;
    let operation = Operation::Batch {
        dimension: dimension.to_owned(),
        changes,
    };
    Ok((entry, preview, bounds, operation))
}

fn operation_bounds(origin: BlockPos, size: [i32; 3]) -> Result<BlockBox, AppError> {
    let max = BlockPos::new(
        origin
            .x
            .checked_add(size[0] - 1)
            .ok_or_else(|| AppError::invalid_argument("template origin overflows X"))?,
        origin
            .y
            .checked_add(size[1] - 1)
            .ok_or_else(|| AppError::invalid_argument("template origin overflows Y"))?,
        origin
            .z
            .checked_add(size[2] - 1)
            .ok_or_else(|| AppError::invalid_argument("template origin overflows Z"))?,
    );
    BlockBox::new(origin, max).map_err(|error| AppError::invalid_argument(error.to_string()))
}

fn template_preview_impl(
    state: &AppState,
    template_id: &str,
    dimension: &str,
    origin: [i32; 3],
    include_air: bool,
) -> Result<TemplateInsertPreview, AppError> {
    let (entry, preview, bounds, operation) =
        prepare_template_insert(state, template_id, dimension, origin, include_air)?;
    let current = current_project(state)?;
    let result = {
        let mut service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        service.set_allow_unknown_targets(true);
        let result = service.preview(&current.project_id, &operation);
        service.set_allow_unknown_targets(false);
        result?
    };
    Ok(TemplateInsertPreview {
        entry,
        preview,
        bounds,
        affected_blocks: result.affected_blocks,
        changed_blocks: result.changed_blocks,
        warnings: result.warnings,
    })
}

fn template_insert_impl(
    state: &AppState,
    template_id: &str,
    dimension: &str,
    origin: [i32; 3],
    include_air: bool,
    layer_id: Option<String>,
) -> Result<StructureInsertOutcome, AppError> {
    let (_entry, preview, _bounds, operation) =
        prepare_template_insert(state, template_id, dimension, origin, include_air)?;
    let current = current_project(state)?;
    let has_changes = matches!(&operation, Operation::Batch { changes, .. } if !changes.is_empty());
    let (operation, info) = {
        let mut service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        let operation = if has_changes {
            service.set_allow_unknown_targets(true);
            let result = service.execute(OperationRequest {
                project_id: current.project_id.clone(),
                operation,
                expected_revision: None,
                idempotency_key: None,
                actor: ui_actor(),
                linked_object_id: None,
                layer_id,
            });
            service.set_allow_unknown_targets(false);
            Some(result?)
        } else {
            None
        };
        (operation, service.world_info())
    };
    let session = session_status(state)?;
    Ok(StructureInsertOutcome {
        preview,
        operation,
        info,
        session,
    })
}

#[tauri::command]
pub fn workspace_policy(state: State<'_, AppState>) -> Result<WorkspacePolicy, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    Ok(service.workspace_policy())
}

#[tauri::command]
pub fn workspace_set_policy(
    state: State<'_, AppState>,
    policy: WorkspacePolicy,
) -> Result<WorkspacePolicy, AppError> {
    workspace_set_policy_impl(&state, policy)
}

fn workspace_set_policy_impl(
    state: &AppState,
    policy: WorkspacePolicy,
) -> Result<WorkspacePolicy, AppError> {
    let current = current_project(state)?;
    // Validate against the authoritative dimensions before touching project.json.
    {
        let service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        service.validate_workspace_policy(&policy)?;
    }
    let mut editor_state = current.editor_state.clone();
    editor_state.workspace = policy.clone();
    editor_state.validate().map_err(project_error)?;
    {
        let mut projects = state
            .projects
            .lock()
            .map_err(|_| AppError::internal("project state lock poisoned"))?;
        projects
            .store
            .update_editor_state(&current.folder, editor_state.clone())
            .map_err(project_error)?;
        if let Some(active) = projects.current.as_mut() {
            if active.folder == current.folder {
                active.editor_state = editor_state;
            }
        }
    }
    let mut service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    service.set_workspace_policy(policy.clone())?;
    Ok(policy)
}

/// Persists editor metadata (workspace policy and/or semantic scene) without rewriting
/// the world snapshot or bumping its revision. The open project keeps the same state so
/// the next command sees the accepted values immediately.
fn persist_editor_state(
    state: &AppState,
    folder: &str,
    editor_state: project_store::EditorState,
) -> Result<(), AppError> {
    let mut projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    projects
        .store
        .update_editor_state(folder, editor_state.clone())
        .map_err(project_error)?;
    if let Some(active) = projects.current.as_mut() {
        if active.folder == folder {
            active.editor_state = editor_state;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn scene_objects(state: State<'_, AppState>) -> Result<Vec<SemanticObject>, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    Ok(service.scene().objects().to_vec())
}

#[tauri::command]
pub fn scene_inspect(
    state: State<'_, AppState>,
    object_id: String,
) -> Result<SceneObjectInspection, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    match service.query(Query::InspectSceneObject { object_id })? {
        QueryResult::SceneObject(inspection) => Ok(inspection),
        _ => Err(AppError::internal("unexpected scene inspection result")),
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn scene_create(
    state: State<'_, AppState>,
    name: String,
    kind: ObjectKind,
    dimension: String,
    bounds: BlockBox,
    parent_id: Option<String>,
    tags: Option<std::collections::BTreeMap<String, String>>,
    note: Option<String>,
) -> Result<SemanticObject, AppError> {
    scene_create_impl(
        &state,
        name,
        kind,
        dimension,
        bounds,
        parent_id,
        tags.unwrap_or_default(),
        note,
    )
}

#[allow(clippy::too_many_arguments)]
fn scene_create_impl(
    state: &AppState,
    name: String,
    kind: ObjectKind,
    dimension: String,
    bounds: BlockBox,
    parent_id: Option<String>,
    tags: std::collections::BTreeMap<String, String>,
    note: Option<String>,
) -> Result<SemanticObject, AppError> {
    let current = current_project(state)?;
    let (object, scene) = {
        let mut service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        let object =
            service.add_scene_object(&name, kind, &dimension, bounds, parent_id, tags, note)?;
        (object, service.scene().clone())
    };
    let mut editor_state = current.editor_state.clone();
    editor_state.scene = scene;
    editor_state.validate().map_err(project_error)?;
    persist_editor_state(state, &current.folder, editor_state)?;
    Ok(object)
}

#[tauri::command]
pub fn scene_delete(
    state: State<'_, AppState>,
    object_id: String,
) -> Result<SemanticObject, AppError> {
    scene_delete_impl(&state, &object_id)
}

fn scene_delete_impl(state: &AppState, object_id: &str) -> Result<SemanticObject, AppError> {
    let current = current_project(state)?;
    let (object, scene) = {
        let mut service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        let object = service.remove_scene_object(object_id)?;
        (object, service.scene().clone())
    };
    let mut editor_state = current.editor_state.clone();
    editor_state.scene = scene;
    editor_state.validate().map_err(project_error)?;
    persist_editor_state(state, &current.folder, editor_state)?;
    Ok(object)
}

/// Persists the authoritative parametric catalog after a recipe change. The world block
/// operation and the recipe update are separate, but both happen under the same command;
/// a persistence failure is returned to the caller instead of being swallowed.
fn persist_current_parametric(
    state: &AppState,
    folder: &str,
    parametric: ParametricScene,
) -> Result<(), AppError> {
    let mut editor_state = {
        let projects = state
            .projects
            .lock()
            .map_err(|_| AppError::internal("project state lock poisoned"))?;
        projects
            .current
            .as_ref()
            .filter(|current| current.folder == folder)
            .map(|current| current.editor_state.clone())
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "the project is no longer open"))?
    };
    editor_state.parametric = parametric;
    editor_state.validate().map_err(project_error)?;
    persist_editor_state(state, folder, editor_state)
}

#[tauri::command]
pub fn parametric_list(
    state: State<'_, AppState>,
) -> Result<Vec<ParametricObjectSummary>, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    service.parametric_summaries()
}

#[tauri::command]
pub fn parametric_inspect(
    state: State<'_, AppState>,
    object_id: String,
) -> Result<ParametricInspection, AppError> {
    parametric_inspect_impl(&state, &object_id)
}

fn parametric_inspect_impl(
    state: &AppState,
    object_id: &str,
) -> Result<ParametricInspection, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    service.inspect_parametric_object(object_id)
}

#[tauri::command]
pub fn parametric_create(
    state: State<'_, AppState>,
    spec: ParametricSpec,
) -> Result<ParametricApplyResult, AppError> {
    parametric_create_impl(&state, spec)
}

fn parametric_create_impl(
    state: &AppState,
    spec: ParametricSpec,
) -> Result<ParametricApplyResult, AppError> {
    let current = current_project(state)?;
    let (result, parametric) = {
        let mut service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        let result = service.create_parametric_object(&current.project_id, ui_actor(), spec)?;
        (result, service.parametric_scene().clone())
    };
    persist_current_parametric(state, &current.folder, parametric)?;
    Ok(result)
}

#[tauri::command]
pub fn parametric_update(
    state: State<'_, AppState>,
    object_id: String,
    spec: ParametricSpec,
    conflict_policy: Option<ParametricConflictPolicy>,
) -> Result<ParametricApplyResult, AppError> {
    parametric_update_impl(
        &state,
        &object_id,
        spec,
        conflict_policy.unwrap_or_default(),
    )
}

fn parametric_update_impl(
    state: &AppState,
    object_id: &str,
    spec: ParametricSpec,
    conflict_policy: ParametricConflictPolicy,
) -> Result<ParametricApplyResult, AppError> {
    let current = current_project(state)?;
    let (result, parametric) = {
        let mut service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        let result = service.update_parametric_object(
            &current.project_id,
            ui_actor(),
            object_id,
            spec,
            conflict_policy,
        )?;
        (result, service.parametric_scene().clone())
    };
    persist_current_parametric(state, &current.folder, parametric)?;
    Ok(result)
}

#[tauri::command]
pub fn parametric_bake(
    state: State<'_, AppState>,
    object_id: String,
) -> Result<ParametricApplyResult, AppError> {
    parametric_bake_impl(&state, &object_id)
}

fn parametric_bake_impl(
    state: &AppState,
    object_id: &str,
) -> Result<ParametricApplyResult, AppError> {
    let current = current_project(state)?;
    let (result, parametric) = {
        let mut service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        let result = service.bake_parametric_object(object_id)?;
        (result, service.parametric_scene().clone())
    };
    persist_current_parametric(state, &current.folder, parametric)?;
    Ok(result)
}

#[tauri::command]
pub fn parametric_rollback(
    state: State<'_, AppState>,
    object_id: String,
    force: Option<bool>,
) -> Result<ParametricApplyResult, AppError> {
    parametric_rollback_impl(&state, &object_id, force.unwrap_or(false))
}

fn parametric_rollback_impl(
    state: &AppState,
    object_id: &str,
    force: bool,
) -> Result<ParametricApplyResult, AppError> {
    let current = current_project(state)?;
    let (result, parametric) = {
        let mut service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        let result = service.rollback_parametric_object(
            &current.project_id,
            ui_actor(),
            object_id,
            force,
        )?;
        (result, service.parametric_scene().clone())
    };
    persist_current_parametric(state, &current.folder, parametric)?;
    Ok(result)
}

#[tauri::command]
pub fn template_list(state: State<'_, AppState>) -> Result<Vec<TemplateEntry>, AppError> {
    current_template_library(&state)?
        .list()
        .map_err(template_error)
}

#[tauri::command]
pub fn template_save_selection(
    state: State<'_, AppState>,
    name: String,
    dimension: String,
    bounds: BlockBox,
) -> Result<TemplateEntry, AppError> {
    let library = current_template_library(&state)?;
    let world = {
        let service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        service.world().clone()
    };
    library
        .save_from_world(&name, &world, &dimension, bounds, TemplateSource::Selection)
        .map_err(template_error)
}

#[tauri::command]
pub fn template_import(
    state: State<'_, AppState>,
    name: String,
    payload: String,
) -> Result<TemplateEntry, AppError> {
    let bytes = decode_structure_payload(&payload)?;
    current_template_library(&state)?
        .save_bytes(&name, &bytes, TemplateSource::Import)
        .map_err(template_error)
}

#[tauri::command]
pub fn template_delete(
    state: State<'_, AppState>,
    template_id: String,
) -> Result<TemplateEntry, AppError> {
    current_template_library(&state)?
        .delete(&template_id)
        .map_err(template_error)
}

#[tauri::command]
pub fn template_preview(
    state: State<'_, AppState>,
    template_id: String,
    dimension: String,
    origin: [i32; 3],
    include_air: bool,
) -> Result<TemplateInsertPreview, AppError> {
    template_preview_impl(&state, &template_id, &dimension, origin, include_air)
}

#[tauri::command]
pub fn template_insert(
    state: State<'_, AppState>,
    template_id: String,
    dimension: String,
    origin: [i32; 3],
    include_air: bool,
    layer_id: Option<String>,
) -> Result<StructureInsertOutcome, AppError> {
    template_insert_impl(
        &state,
        &template_id,
        &dimension,
        origin,
        include_air,
        layer_id,
    )
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
                layer_id: None,
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
            target: outcome.project.target.clone(),
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
                layer_id: None,
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
    let (bundle, revision, checksum, world_version) = {
        let service = state
            .service
            .lock()
            .map_err(|_| AppError::internal("world state lock poisoned"))?;
        let bundle = world_io::prepare_export(service.world(), &dimension, bounds, "FuryBuilder")
            .map_err(structure_error)?;
        let revision = service.world().revision();
        let checksum = project_store::checksum(&bundle.bytes);
        let version = service.world().version().clone();
        (bundle, revision, checksum, version)
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
        target_data_version: world_version.data_version(),
        target_minecraft_version: world_version.minecraft_version().to_owned(),
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
    let target = world_io::profile_for(data_version).ok_or_else(|| {
        AppError::new(
            ErrorCode::UnsupportedVersion,
            format!("no version profile owns DataVersion {data_version}"),
        )
    })?;
    Ok(StructureExportResult {
        outcome,
        verified: true,
        size,
        non_air_blocks,
        data_version,
        target_data_version: target.data_version,
        target_minecraft_version: target.minecraft_version.to_string(),
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
pub fn assets_saved_source(components: State<'_, ComponentManager>) -> SavedAssetSource {
    components.assets().saved_source()
}

#[tauri::command]
pub fn assets_save_source(
    app: AppHandle,
    components: State<'_, ComponentManager>,
    kind: String,
    path: String,
) -> Result<SavedAssetSource, AppError> {
    let expected = match kind.as_str() {
        "minecraft_root" => AssetSelectionKind::MinecraftRoot,
        "version_directory" => AssetSelectionKind::VersionDirectory,
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
            asset_source::target_minecraft_version()
        ))
        .with_details(details()));
    }
    components
        .assets()
        .save(AssetSourceSetting { kind, path })
        .map_err(asset_settings_error)
        .inspect(|_| {
            let _ = app.emit("furybuilder-assets-changed", ());
        })
}

#[tauri::command]
pub fn assets_use_placeholder(
    app: AppHandle,
    components: State<'_, ComponentManager>,
) -> Result<SavedAssetSource, AppError> {
    components
        .assets()
        .use_placeholder()
        .map_err(asset_settings_error)
        .inspect(|_| {
            let _ = app.emit("furybuilder-assets-changed", ());
        })
}

#[tauri::command]
pub fn assets_clear_source(
    app: AppHandle,
    components: State<'_, ComponentManager>,
) -> Result<SavedAssetSource, AppError> {
    components
        .assets()
        .clear()
        .map_err(asset_settings_error)
        .inspect(|_| {
            let _ = app.emit("furybuilder-assets-changed", ());
        })
}

#[tauri::command]
pub fn assets_save_pack_stack(
    app: AppHandle,
    components: State<'_, ComponentManager>,
    paths: Vec<String>,
) -> Result<SavedAssetSource, AppError> {
    components
        .assets()
        .save_pack_stack(paths)
        .map_err(asset_settings_error)
        .inspect(|_| {
            let _ = app.emit("furybuilder-assets-changed", ());
        })
}

#[tauri::command]
pub fn assets_clear_pack_stack(
    app: AppHandle,
    components: State<'_, ComponentManager>,
) -> Result<SavedAssetSource, AppError> {
    components
        .assets()
        .clear_pack_stack()
        .map_err(asset_settings_error)
        .inspect(|_| {
            let _ = app.emit("furybuilder-assets-changed", ());
        })
}

#[tauri::command]
pub fn assets_render_bundle(
    components: State<'_, ComponentManager>,
    service: State<'_, AssetBundleService>,
    block_states: Vec<BlockStateRequest>,
    source_kind: Option<String>,
    source_path: Option<String>,
) -> Result<AssetBundle, AppError> {
    let source = resolve_asset_source(components.assets(), source_kind, source_path)?.ok_or_else(
        || {
            AppError::invalid_argument(
                "select Minecraft client assets before rendering Minecraft models; placeholder textures are used meanwhile",
            )
        },
    )?;
    service
        .bundle(
            Some(&source),
            &components.assets().pack_stack(),
            &block_states,
        )
        .map_err(asset_bundle_error)
}

/// Builds the block/item catalog from the selected client JAR. No files are downloaded or
/// written; callers must have an explicit asset source.
#[tauri::command]
pub fn assets_catalog(
    components: State<'_, ComponentManager>,
    service: State<'_, AssetCatalogService>,
    source_kind: Option<String>,
    source_path: Option<String>,
) -> Result<CatalogSnapshot, AppError> {
    let saved = components.assets().saved_source();
    let source = resolve_asset_source(components.assets(), source_kind, source_path)?.ok_or_else(
        || {
            if saved.placeholder {
                AppError::invalid_argument(
                    "placeholder textures have no block/item catalog; select or download a Minecraft client JAR",
                )
            } else {
                AppError::invalid_argument(
                    "select Minecraft client assets before building a block/item catalog",
                )
            }
        },
    )?;
    service.catalog(&source).map_err(asset_catalog_error)
}

#[tauri::command]
pub fn assets_clear_cache(
    service: State<'_, AssetBundleService>,
    catalog: State<'_, AssetCatalogService>,
) {
    service.clear_cache();
    catalog.clear();
}

fn resolve_asset_source(
    store: &AssetSettingsStore,
    source_kind: Option<String>,
    source_path: Option<String>,
) -> Result<Option<AssetSourceSetting>, AppError> {
    match (source_kind, source_path) {
        (Some(kind), Some(path)) => Ok(Some(AssetSourceSetting { kind, path })),
        (None, None) => Ok(store.saved_source().source),
        _ => Err(AppError::invalid_argument(
            "sourceKind and sourcePath must be provided together",
        )),
    }
}

/// Event payload for a completed mutation. The legacy kind-specific events are emitted too
/// until every panel reads the unified component state.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentChangedEvent {
    pub kind: ComponentKind,
}

/// Event payload for a failed component mutation. `code` matches the rejected command error.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentErrorEvent {
    pub kind: ComponentKind,
    pub code: String,
    pub message: String,
}

fn emit_component_changed(app: &AppHandle, kind: ComponentKind) {
    let _ = app.emit(
        "furybuilder-component-changed",
        ComponentChangedEvent { kind },
    );
    let legacy = match kind {
        ComponentKind::Java => "furybuilder-runtime-changed",
        ComponentKind::Assets => "furybuilder-assets-changed",
        ComponentKind::Server => "furybuilder-server-changed",
    };
    let _ = app.emit(legacy, ());
}

fn component_app_error(app: &AppHandle, kind: ComponentKind, error: ComponentError) -> AppError {
    let _ = app.emit(
        "furybuilder-component-error",
        ComponentErrorEvent {
            kind,
            code: error.code().to_owned(),
            message: error.to_string(),
        },
    );
    component_error(error)
}

/// Reports what the target needs and what is installed. Read-only.
#[tauri::command]
pub async fn components_status(
    components: State<'_, ComponentManager>,
    target: ComponentTarget,
    requirements: ComponentRequirements,
) -> Result<ComponentReport, AppError> {
    let components = components.inner().clone();
    tauri::async_runtime::spawn_blocking(move || components.status(&target, &requirements))
        .await
        .map_err(|error| AppError::internal(format!("component status task failed: {error}")))?
        .map_err(component_error)
}

/// Returns the verified path, id and hash of one ready component.
#[tauri::command]
pub async fn components_resolve(
    components: State<'_, ComponentManager>,
    kind: ComponentKind,
    target: ComponentTarget,
    requirements: ComponentRequirements,
) -> Result<ResolvedComponent, AppError> {
    let components = components.inner().clone();
    tauri::async_runtime::spawn_blocking(move || components.resolve(kind, &target, &requirements))
        .await
        .map_err(|error| AppError::internal(format!("component resolve task failed: {error}")))?
        .map_err(component_error)
}

/// Lists every FuryBuilder-owned component artifact. Read-only.
#[tauri::command]
pub async fn components_inventory(
    components: State<'_, ComponentManager>,
) -> Result<ComponentInventory, AppError> {
    let components = components.inner().clone();
    tauri::async_runtime::spawn_blocking(move || components.inventory())
        .await
        .map_err(|error| AppError::internal(format!("component inventory task failed: {error}")))?
        .map_err(component_error)
}

/// Adoptium release preview for the target Java major. No bytes are downloaded.
#[tauri::command]
pub async fn components_java_release(
    components: State<'_, ComponentManager>,
    target: ComponentTarget,
) -> Result<RuntimeReleaseInfo, AppError> {
    let components = components.inner().clone();
    tauri::async_runtime::spawn_blocking(move || components.java_release(&target))
        .await
        .map_err(|error| AppError::internal(format!("Java release task failed: {error}")))?
        .map_err(component_error)
}

/// Official client JAR preview for the target version. No bytes are downloaded.
#[tauri::command]
pub async fn components_assets_release(
    components: State<'_, ComponentManager>,
    target: ComponentTarget,
) -> Result<ClientJarReleaseInfo, AppError> {
    let components = components.inner().clone();
    tauri::async_runtime::spawn_blocking(move || components.assets_release(&target))
        .await
        .map_err(|error| AppError::internal(format!("client JAR release task failed: {error}")))?
        .map_err(component_error)
}

/// Official server JAR preview for the exact target version. No bytes are downloaded.
#[tauri::command]
pub async fn components_server_release(
    components: State<'_, ComponentManager>,
    target: ComponentTarget,
) -> Result<ServerReleaseInfo, AppError> {
    let components = components.inner().clone();
    tauri::async_runtime::spawn_blocking(move || components.server_release(&target))
        .await
        .map_err(|error| AppError::internal(format!("server release task failed: {error}")))?
        .map_err(component_error)
}

/// Explicit install of one component. Progress is streamed as
/// `furybuilder-component-progress`; a failure emits `furybuilder-component-error` before
/// the command rejects with the same code.
#[tauri::command]
pub async fn components_install(
    app: AppHandle,
    components: State<'_, ComponentManager>,
    request: ComponentInstallRequest,
) -> Result<ResolvedComponent, AppError> {
    let components = components.inner().clone();
    let kind = request.kind;
    let progress_app = app.clone();
    let last = std::sync::Mutex::new(None::<(String, std::time::Instant)>);
    let installed = tauri::async_runtime::spawn_blocking(move || {
        components.install(&request, &|progress: ComponentProgress| {
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
                let _ = progress_app.emit("furybuilder-component-progress", &progress);
            }
        })
    })
    .await
    .map_err(|error| AppError::internal(format!("component install task failed: {error}")))?;
    let installed = installed.map_err(|error| component_app_error(&app, kind, error))?;
    emit_component_changed(&app, kind);
    Ok(installed)
}

/// Requests cancellation of the active install for this kind. The install command reports
/// the cancellation and removes its partial files.
#[tauri::command]
pub fn components_cancel(
    components: State<'_, ComponentManager>,
    kind: ComponentKind,
) -> CancelOutcome {
    components.cancel(kind)
}

/// Validates and records a manual component source. The user's file is never copied or
/// edited, and only a verified source becomes ready.
#[tauri::command]
pub async fn components_relink(
    app: AppHandle,
    components: State<'_, ComponentManager>,
    kind: ComponentKind,
    path: String,
    target: ComponentTarget,
) -> Result<ComponentStatus, AppError> {
    let components = components.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        components.relink(kind, std::path::Path::new(&path), &target)
    })
    .await
    .map_err(|error| AppError::internal(format!("component relink task failed: {error}")))?;
    let status = result.map_err(|error| component_app_error(&app, kind, error))?;
    emit_component_changed(&app, kind);
    Ok(status)
}

/// Explicitly selects placeholder textures for the asset component. No client JAR, catalog or
/// checksum is claimed, and the resulting status is never ready.
#[tauri::command]
pub async fn components_use_placeholder(
    app: AppHandle,
    components: State<'_, ComponentManager>,
    target: ComponentTarget,
) -> Result<ComponentStatus, AppError> {
    let components = components.inner().clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || components.use_placeholder_assets(&target))
            .await
            .map_err(|error| {
                AppError::internal(format!("component placeholder task failed: {error}"))
            })?;
    let status = result.map_err(|error| component_app_error(&app, ComponentKind::Assets, error))?;
    emit_component_changed(&app, ComponentKind::Assets);
    Ok(status)
}

/// Removes FuryBuilder-owned data or clears a manual pointer. A user file outside the
/// managed cache is never deleted.
#[tauri::command]
pub async fn components_remove(
    app: AppHandle,
    components: State<'_, ComponentManager>,
    kind: ComponentKind,
    id: String,
) -> Result<ComponentRemoval, AppError> {
    let components = components.inner().clone();
    let result = tauri::async_runtime::spawn_blocking(move || components.remove(kind, &id))
        .await
        .map_err(|error| AppError::internal(format!("component remove task failed: {error}")))?;
    let removal = result.map_err(|error| component_app_error(&app, kind, error))?;
    emit_component_changed(&app, kind);
    Ok(removal)
}

/// Re-inspects the selected component and returns fresh facts. A stale or unknown id is
/// refused instead of verifying another file.
#[tauri::command]
pub async fn components_verify(
    components: State<'_, ComponentManager>,
    kind: ComponentKind,
    id: String,
    target: ComponentTarget,
) -> Result<ComponentVerification, AppError> {
    let components = components.inner().clone();
    tauri::async_runtime::spawn_blocking(move || components.verify(kind, &id, &target))
        .await
        .map_err(|error| AppError::internal(format!("component verify task failed: {error}")))?
        .map_err(component_error)
}

#[tauri::command]
pub async fn runtime_discover(
    components: State<'_, ComponentManager>,
) -> Result<RuntimeDiscoveryReport, AppError> {
    let manager = components.java().clone();
    tauri::async_runtime::spawn_blocking(move || manager.discover())
        .await
        .map_err(|error| AppError::internal(format!("runtime discovery task failed: {error}")))
}

#[tauri::command]
pub fn runtime_saved(components: State<'_, ComponentManager>) -> Option<JavaRuntimeReport> {
    components.java().saved()
}

#[tauri::command]
pub async fn runtime_select_java(
    app: AppHandle,
    components: State<'_, ComponentManager>,
    path: String,
) -> Result<JavaRuntimeReport, AppError> {
    let manager = components.java().clone();
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
    components: State<'_, ComponentManager>,
) -> Result<(), AppError> {
    components.java().clear_selection().map_err(runtime_error)?;
    let _ = app.emit("furybuilder-runtime-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn runtime_temurin_latest(
    components: State<'_, ComponentManager>,
    major: u32,
) -> Result<RuntimeReleaseInfo, AppError> {
    let manager = components.java().clone();
    tauri::async_runtime::spawn_blocking(move || manager.latest_temurin(major))
        .await
        .map_err(|error| AppError::internal(format!("Adoptium request task failed: {error}")))?
        .map_err(runtime_error)
}

#[tauri::command]
pub async fn runtime_install_temurin(
    app: AppHandle,
    components: State<'_, ComponentManager>,
    major: u32,
    version: String,
) -> Result<InstalledRuntime, AppError> {
    let manager = components.java().clone();
    let progress_app = app.clone();
    let last = std::sync::Mutex::new(None::<(String, std::time::Instant)>);
    let installed = tauri::async_runtime::spawn_blocking(move || {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        manager.install_temurin(major, &version, &cancel, &|progress: RuntimeProgress| {
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
    components: State<'_, ComponentManager>,
) -> Result<ManagedCacheReport, AppError> {
    let manager = components.java().clone();
    tauri::async_runtime::spawn_blocking(move || manager.managed_list())
        .await
        .map_err(|error| AppError::internal(format!("runtime cache task failed: {error}")))?
        .map_err(runtime_error)
}

#[tauri::command]
pub async fn runtime_clear_managed(
    app: AppHandle,
    components: State<'_, ComponentManager>,
) -> Result<ManagedCacheReport, AppError> {
    let manager = components.java().clone();
    let report = tauri::async_runtime::spawn_blocking(move || manager.clear_managed())
        .await
        .map_err(|error| AppError::internal(format!("runtime cache task failed: {error}")))?
        .map_err(runtime_error)?;
    let _ = app.emit("furybuilder-runtime-changed", ());
    Ok(report)
}

#[tauri::command]
pub async fn server_discover(
    components: State<'_, ComponentManager>,
) -> Result<ServerDiscoveryReport, AppError> {
    let manager = components.server().clone();
    tauri::async_runtime::spawn_blocking(move || manager.discover())
        .await
        .map_err(|error| AppError::internal(format!("server discovery task failed: {error}")))
}

#[tauri::command]
pub fn server_saved(components: State<'_, ComponentManager>) -> Option<ServerJarReport> {
    components
        .server()
        .selected(&runtime_manager::ServerTarget::baseline())
}

#[tauri::command]
pub async fn server_select_jar(
    app: AppHandle,
    components: State<'_, ComponentManager>,
    path: String,
) -> Result<ServerJarReport, AppError> {
    let manager = components.server().clone();
    let report = tauri::async_runtime::spawn_blocking(move || {
        manager.select_manual(
            std::path::Path::new(&path),
            &runtime_manager::ServerTarget::baseline(),
        )
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
    components: State<'_, ComponentManager>,
) -> Result<(), AppError> {
    let target = runtime_manager::ServerTarget::baseline();
    components
        .server()
        .clear_selection(&target.minecraft_version)
        .map_err(server_error)?;
    let _ = app.emit("furybuilder-server-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn server_release(
    components: State<'_, ComponentManager>,
) -> Result<ServerReleaseInfo, AppError> {
    let manager = components.server().clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.release(&runtime_manager::ServerTarget::baseline())
    })
    .await
    .map_err(|error| AppError::internal(format!("Mojang metadata task failed: {error}")))?
    .map_err(server_error)
}

#[tauri::command]
pub async fn server_install(
    app: AppHandle,
    components: State<'_, ComponentManager>,
    version: String,
) -> Result<InstalledServerJar, AppError> {
    let target = runtime_manager::ServerTarget::baseline();
    if version != target.minecraft_version.as_str() {
        return Err(AppError::invalid_argument(format!(
            "The server settings page can install Minecraft {}; project-specific versions use the component installer.",
            target.minecraft_version
        )));
    }
    let manager = components.server().clone();
    let progress_app = app.clone();
    let last = std::sync::Mutex::new(None::<(String, std::time::Instant)>);
    let installed = tauri::async_runtime::spawn_blocking(move || {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        manager.install(&target, &cancel, &|progress: ServerProgress| {
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
    components: State<'_, ComponentManager>,
) -> Result<ManagedServerCacheReport, AppError> {
    let manager = components.server().clone();
    tauri::async_runtime::spawn_blocking(move || manager.managed_list())
        .await
        .map_err(|error| AppError::internal(format!("server cache task failed: {error}")))?
        .map_err(server_error)
}

#[tauri::command]
pub async fn server_clear_managed(
    app: AppHandle,
    components: State<'_, ComponentManager>,
) -> Result<ManagedServerCacheReport, AppError> {
    let manager = components.server().clone();
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
    components: State<'_, ComponentManager>,
    accepted: bool,
) -> Result<SavedEula, AppError> {
    let eula = components
        .server()
        .accept_eula(accepted, &runtime_manager::ServerTarget::baseline())
        .map_err(server_error)?;
    let _ = app.emit("furybuilder-server-changed", ());
    Ok(eula)
}

#[tauri::command]
pub fn server_clear_eula(
    app: AppHandle,
    components: State<'_, ComponentManager>,
) -> Result<(), AppError> {
    components.server().clear_eula().map_err(server_error)?;
    let _ = app.emit("furybuilder-server-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn server_start_generation(
    app: AppHandle,
    components: State<'_, ComponentManager>,
    request: GenerationRequest,
) -> Result<GenerationSnapshot, AppError> {
    let runtime = components.java().clone();
    let java = tauri::async_runtime::spawn_blocking(move || runtime.saved())
        .await
        .map_err(|error| AppError::internal(format!("Java runtime task failed: {error}")))?;
    let Some(java) = java else {
        return Err(AppError::new(
            ErrorCode::InvalidArgument,
            format!(
                "Select a usable Java {} runtime before starting generation.",
                runtime_manager::target_java_major()
            ),
        ));
    };
    if !java.is_usable() {
        return Err(AppError::new(
            ErrorCode::InvalidArgument,
            format!(
                "The saved Java runtime is no longer usable with Minecraft {}.",
                runtime_manager::target_minecraft_version()
            ),
        ));
    }
    let manager = components.server().clone();
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
pub fn server_generation_status(components: State<'_, ComponentManager>) -> GenerationStatusReport {
    components.server().generation_status()
}

/// Bounded preflight for the Source step: expected area, conservative world size and the
/// free space on the volume that owns the isolated run directory.
#[tauri::command]
pub async fn server_generation_estimate(
    components: State<'_, ComponentManager>,
    radius_chunks: u32,
    time_limit_seconds: u32,
) -> Result<GenerationEstimate, AppError> {
    let manager = components.server().clone();
    tauri::async_runtime::spawn_blocking(move || {
        manager.generation_estimate(radius_chunks, time_limit_seconds)
    })
    .await
    .map_err(|error| AppError::internal(format!("generation estimate task failed: {error}")))?
    .map_err(server_error)
}

#[tauri::command]
pub fn server_cancel_generation(components: State<'_, ComponentManager>) -> bool {
    components.server().cancel_generation()
}

#[tauri::command]
pub async fn server_generation_runs(
    components: State<'_, ComponentManager>,
) -> Result<GenerationRunsReport, AppError> {
    let manager = components.server().clone();
    tauri::async_runtime::spawn_blocking(move || manager.generation_runs())
        .await
        .map_err(|error| AppError::internal(format!("generation runs task failed: {error}")))
}

#[tauri::command]
pub async fn server_clear_generation_runs(
    app: AppHandle,
    components: State<'_, ComponentManager>,
) -> Result<GenerationRunsReport, AppError> {
    let manager = components.server().clone();
    let report = tauri::async_runtime::spawn_blocking(move || manager.clear_generation_runs())
        .await
        .map_err(|error| AppError::internal(format!("generation runs task failed: {error}")))?;
    let report = report.map_err(server_error)?;
    let _ = app.emit("furybuilder-server-changed", ());
    Ok(report)
}

#[tauri::command]
pub async fn server_inspect_datapacks(
    components: State<'_, ComponentManager>,
    paths: Vec<String>,
) -> Result<Vec<DatapackInspection>, AppError> {
    if paths.len() > 16 {
        return Err(AppError::new(
            ErrorCode::LimitExceeded,
            "At most 16 data packs can be inspected at once.",
        ));
    }
    let manager = components.server().clone();
    tauri::async_runtime::spawn_blocking(move || manager.inspect_datapacks(&paths))
        .await
        .map_err(|error| AppError::internal(format!("data pack inspection task failed: {error}")))
}

/// Combined Vanilla World setup check for the wizard. It reads the already verified runtime
/// and server selections; it never probes, downloads, accepts the EULA or starts anything.
#[tauri::command]
pub async fn vanilla_setup_check(
    components: State<'_, ComponentManager>,
) -> Result<VanillaSetupReport, AppError> {
    let runtime = components.java().clone();
    let java = tauri::async_runtime::spawn_blocking(move || runtime.saved())
        .await
        .map_err(|error| AppError::internal(format!("Java runtime task failed: {error}")))?;
    let java_ready = java
        .as_ref()
        .map(JavaRuntimeReport::is_compatible)
        .unwrap_or(false);
    let discovery = components.server().discover();
    let server_ready = discovery
        .selected
        .as_ref()
        .map(|server| server.ready)
        .unwrap_or(false);
    let eula_matches_selected = discovery.eula_matches_selected;
    let eula_accepted_at = discovery.eula.as_ref().map(|eula| eula.accepted_at);
    let profile = world_io::default_version_profile();
    let mut blockers = Vec::new();
    match java.as_ref() {
        None => blockers.push(format!(
            "Select a compatible Java {} runtime (Settings → Java runtime or this wizard).",
            profile.java.recommended_major
        )),
        Some(_) if !java_ready => blockers.push(format!(
            "The saved Java runtime is not compatible with Minecraft Java {}.",
            profile.minecraft_version
        )),
        Some(_) => {}
    }
    match discovery.selected.as_ref() {
        None => blockers.push(format!(
            "Select or download the official Minecraft {} server.jar.",
            profile.minecraft_version
        )),
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
        target_minecraft_version: profile.minecraft_version.to_string(),
        target_data_version: profile.data_version,
        target_java_major: profile.java.recommended_major,
        server_datapack_format: runtime_manager::target_datapack_format(),
        generation_limits: components.server().generation_limits(),
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
    pub generation_limits: GenerationLimits,
    pub java: Option<JavaRuntimeReport>,
    pub java_ready: bool,
    pub server: Option<ServerJarReport>,
    pub server_ready: bool,
    pub eula_accepted_at: Option<u64>,
    pub eula_matches_selected: bool,
    pub blockers: Vec<String>,
    pub ready: bool,
}

fn component_error(error: ComponentError) -> AppError {
    let code = match error.code() {
        "invalid_argument" => ErrorCode::InvalidArgument,
        "unsupported_version" => ErrorCode::UnsupportedVersion,
        "not_ready" => ErrorCode::Conflict,
        "not_found" => ErrorCode::NotFound,
        "busy" => ErrorCode::Conflict,
        "cancelled" => ErrorCode::Cancelled,
        "checksum_mismatch" => ErrorCode::Conflict,
        "network_error" => ErrorCode::BackendUnavailable,
        "offline" => ErrorCode::Offline,
        "disk_space" => ErrorCode::DiskSpace,
        "unsupported_format" => ErrorCode::UnsupportedFormat,
        "limit_exceeded" => ErrorCode::LimitExceeded,
        _ => ErrorCode::Internal,
    };
    AppError::new(code, error.to_string())
}

fn server_error(error: runtime_manager::ServerError) -> AppError {
    let code = match error.code() {
        "invalid_argument" => ErrorCode::InvalidArgument,
        "unsupported_version" => ErrorCode::UnsupportedVersion,
        "unsupported_runtime" => ErrorCode::UnsupportedVersion,
        "eula_required" => ErrorCode::PermissionDenied,
        "consent_required" => ErrorCode::PermissionDenied,
        "network_error" => ErrorCode::BackendUnavailable,
        "offline" => ErrorCode::Offline,
        "checksum_mismatch" => ErrorCode::Conflict,
        "disk_space" => ErrorCode::DiskSpace,
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
        "offline" => ErrorCode::Offline,
        "checksum_mismatch" => ErrorCode::Conflict,
        "disk_space" => ErrorCode::DiskSpace,
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

fn asset_catalog_error(error: asset_source::CatalogError) -> AppError {
    let code = match error.code() {
        "invalid_argument" => ErrorCode::InvalidArgument,
        "unsupported_format" | "invalid_version" => ErrorCode::UnsupportedFormat,
        "limit_exceeded" => ErrorCode::LimitExceeded,
        _ => ErrorCode::Internal,
    };
    AppError::new(code, error.to_string())
}

#[cfg(test)]
mod version_catalog_tests {
    use super::*;
    use minecraft_version::{CatalogSource, VersionCatalog, VersionManifest};

    const MANIFEST: &str = r#"{
      "latest": { "release": "1.21.5", "snapshot": "26.1-snapshot-3" },
      "versions": [
        {
          "id": "1.21.5",
          "type": "release",
          "url": "https://piston-meta.mojang.com/v1/packages/aaa/1.21.5.json",
          "time": "2025-03-25T10:00:00+00:00",
          "releaseTime": "2025-03-25T10:00:00+00:00",
          "sha1": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        },
        {
          "id": "26.1-snapshot-3",
          "type": "snapshot",
          "url": "https://piston-meta.mojang.com/v1/packages/bbb/26.1-snapshot-3.json",
          "time": "2025-12-10T09:00:00+00:00",
          "releaseTime": "2025-12-10T09:00:00+00:00",
          "sha1": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        },
        {
          "id": "1.21.4",
          "type": "release",
          "url": "https://piston-meta.mojang.com/v1/packages/ccc/1.21.4.json",
          "time": "2024-12-03T10:12:57+00:00",
          "releaseTime": "2024-12-03T10:12:57+00:00",
          "sha1": "cccccccccccccccccccccccccccccccccccccccc"
        }
      ]
    }"#;

    fn report() -> VersionCatalogReport {
        let manifest = VersionManifest::from_json(MANIFEST).expect("manifest");
        let catalog =
            VersionCatalog::from_manifest(manifest, 1_700_000_000, CatalogSource::Network)
                .expect("catalog");
        build_version_catalog_report(&catalog)
    }

    #[test]
    fn report_marks_latest_and_recommended_without_promising_unsupported_profiles() {
        let report = report();
        assert_eq!(report.source, "network");
        assert_eq!(report.latest_release, "1.21.5");
        assert_eq!(report.latest_snapshot, "26.1-snapshot-3");
        assert_eq!(report.recommended, "1.21.4");
        assert_eq!(report.versions.len(), 3);

        let latest = &report.versions[0];
        assert_eq!(latest.id, "1.21.5");
        assert!(latest.latest_release);
        assert!(!latest.recommended);
        assert!(latest.profile.is_none());
        assert!(latest
            .unavailable_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("no verified profile")));

        let snapshot = &report.versions[1];
        assert_eq!(snapshot.id, "26.1-snapshot-3");
        assert!(snapshot.latest_snapshot);
        assert!(snapshot.profile.is_none());
        assert!(snapshot
            .unavailable_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("snapshot")));

        let recommended = &report.versions[2];
        assert_eq!(recommended.id, "1.21.4");
        assert!(recommended.recommended);
        let profile = recommended.profile.as_ref().expect("verified profile");
        assert_eq!(profile.id, minecraft_version::JAVA_CLASSIC_1_21_4_ID);
        assert_eq!(
            profile.capabilities.support_level,
            minecraft_version::SupportLevel::Full
        );
        assert!(recommended.unavailable_reason.is_none());
    }

    #[test]
    fn report_serializes_the_profile_as_camel_case() {
        let value = serde_json::to_value(report()).expect("json");
        assert_eq!(value["versions"][2]["versionType"], "release");
        assert_eq!(
            value["versions"][2]["profile"]["minecraftVersion"],
            "1.21.4"
        );
        assert_eq!(
            value["versions"][2]["profile"]["java"]["recommendedMajor"],
            21
        );
        assert_eq!(
            value["versions"][2]["profile"]["capabilities"]["supportLevel"],
            "full"
        );
    }
}

#[cfg(test)]
mod workspace_tests {
    use super::*;
    use project_store::CreationMode;

    fn project_state(label: &str) -> (AppState, String) {
        let root = std::env::temp_dir().join(format!(
            "furybuilder-commands-{label}-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let state = AppState::new(root);
        let (folder, detail, world) = {
            let mut projects = state.projects.lock().expect("projects");
            let created = projects
                .store
                .create(
                    "Workspace",
                    CreationMode::Flat,
                    project_store::EditorState::default(),
                )
                .expect("create");
            let folder = created.summary.folder.clone();
            let (detail, world, _) = projects.store.open(&folder).expect("open");
            projects.current = Some(CurrentProject {
                folder: folder.clone(),
                project_id: detail.summary.project_id.clone(),
                name: detail.summary.name.clone(),
                target: detail.target.clone(),
                saved_revision: detail.summary.world_revision,
                autosaved_revision: None,
                editor_state: detail.editor_state.clone(),
            });
            (folder, detail, world)
        };
        let _ = detail;
        install_world_service(&state, world, &project_store::EditorState::default())
            .expect("service");
        (state, folder)
    }

    fn floor_template_bytes() -> Vec<u8> {
        let stone: BlockState = "minecraft:stone".parse().expect("state");
        let structure = Structure::new(
            world_io::default_version_profile().data_version,
            [2, 1, 2],
            vec![stone],
            vec![
                world_io::StructureBlock {
                    pos: [0, 0, 0],
                    palette_index: 0,
                },
                world_io::StructureBlock {
                    pos: [1, 0, 0],
                    palette_index: 0,
                },
                world_io::StructureBlock {
                    pos: [0, 0, 1],
                    palette_index: 0,
                },
                world_io::StructureBlock {
                    pos: [1, 0, 1],
                    palette_index: 0,
                },
            ],
        )
        .expect("structure");
        world_io::write_java_structure(&structure, "Test").expect("bytes")
    }

    fn save_template(state: &AppState) -> TemplateEntry {
        current_template_library(state)
            .expect("library")
            .save_bytes("Floor", &floor_template_bytes(), TemplateSource::Selection)
            .expect("save")
    }

    #[test]
    fn template_insert_is_one_reversible_batch_on_the_active_layer() {
        let (state, folder) = project_state("template-insert");
        let initial_revision = state.service.lock().expect("service").world_info().revision;
        let previous = state
            .service
            .lock()
            .expect("service")
            .world()
            .get_block("minecraft:overworld", BlockPos::new(4, 64, 4))
            .expect("previous block");
        let entry = save_template(&state);
        assert_eq!(entry.non_air_blocks, 4);

        let outcome = template_insert_impl(
            &state,
            &entry.id,
            "minecraft:overworld",
            [4, 64, 4],
            false,
            None,
        )
        .expect("insert");
        let operation = outcome.operation.expect("operation");
        assert_eq!(operation.world_revision, initial_revision + 1);
        assert_eq!(operation.changed_blocks, 4);
        {
            let service = state.service.lock().expect("service");
            let installed = service
                .world()
                .get_block("minecraft:overworld", BlockPos::new(4, 64, 4))
                .expect("block");
            assert_eq!(installed, "minecraft:stone".parse().expect("state"));
            assert_eq!(
                service
                    .history()
                    .last()
                    .expect("history")
                    .layer_id
                    .as_deref(),
                Some("base")
            );
        }
        let undone = {
            let mut service = state.service.lock().expect("service");
            service
                .undo("workspace", Actor::ui("test"), None)
                .expect("undo")
        };
        assert_eq!(undone.world_revision, initial_revision + 2);
        assert_eq!(
            state
                .service
                .lock()
                .expect("service")
                .world()
                .get_block("minecraft:overworld", BlockPos::new(4, 64, 4))
                .expect("block"),
            previous
        );

        // The library is project-local and survives reopening the project.
        let reopened = state
            .projects
            .lock()
            .expect("projects")
            .store
            .open(&folder)
            .expect("reopen");
        assert_eq!(
            reopened.0.editor_state.workspace,
            WorkspacePolicy::default()
        );
        let library = StructureLibrary::new(
            state
                .projects
                .lock()
                .expect("projects")
                .store
                .templates_dir(&folder)
                .expect("templates"),
        );
        assert_eq!(library.list().expect("list").len(), 1);
    }

    #[test]
    fn protected_regions_and_locked_layers_block_template_inserts_and_persist() {
        let (state, folder) = project_state("template-protection");
        let initial_revision = state.service.lock().expect("service").world_info().revision;
        let entry = save_template(&state);

        let mut policy = WorkspacePolicy::default();
        policy
            .push_protected_region(
                "Spawn",
                "minecraft:overworld",
                BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(8, 70, 8)).expect("box"),
            )
            .expect("protected");
        workspace_set_policy_impl(&state, policy).expect("policy");
        let error = template_insert_impl(
            &state,
            &entry.id,
            "minecraft:overworld",
            [4, 64, 4],
            false,
            None,
        )
        .expect_err("protected");
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert!(error.message.contains("protected region `Spawn`"));
        assert_eq!(
            state.service.lock().expect("service").world_info().revision,
            initial_revision
        );

        let reopened = state
            .projects
            .lock()
            .expect("projects")
            .store
            .open(&folder)
            .expect("reopen");
        assert_eq!(
            reopened.0.editor_state.workspace.protected_regions[0].name,
            "Spawn"
        );

        let mut policy = reopened.0.editor_state.workspace;
        policy.layers[0].locked = true;
        workspace_set_policy_impl(&state, policy).expect("locked policy");
        let error = template_insert_impl(
            &state,
            &entry.id,
            "minecraft:overworld",
            [4, 64, 4],
            false,
            None,
        )
        .expect_err("locked");
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert!(error.message.contains("is locked"));

        // A policy update is metadata: it does not bump the world revision.
        assert_eq!(
            state.service.lock().expect("service").world_info().revision,
            initial_revision
        );
    }

    #[test]
    fn scene_objects_persist_and_inspect_blocks_without_world_edits() {
        use std::collections::BTreeMap;
        use world_application::ObjectKind;

        let (state, folder) = project_state("scene");
        let project_id = current_project(&state).expect("current").project_id;
        let initial_revision = state.service.lock().expect("service").world_info().revision;
        {
            let mut service = state.service.lock().expect("service");
            service
                .execute(OperationRequest {
                    project_id: project_id.clone(),
                    operation: Operation::SetBlock {
                        dimension: "minecraft:overworld".to_owned(),
                        pos: BlockPos::new(4, 64, 4),
                        state: "minecraft:stone".parse().expect("state"),
                    },
                    expected_revision: None,
                    idempotency_key: None,
                    actor: ui_actor(),
                    linked_object_id: None,
                    layer_id: None,
                })
                .expect("stone");
        }
        let revision_after_block = state.service.lock().expect("service").world_info().revision;
        assert_eq!(revision_after_block, initial_revision + 1);

        let district = scene_create_impl(
            &state,
            "Old Town".to_owned(),
            ObjectKind::District,
            "minecraft:overworld".to_owned(),
            BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(16, 80, 16)).expect("bounds"),
            None,
            BTreeMap::from([("style".to_owned(), "medieval".to_owned())]),
            Some("Historic center".to_owned()),
        )
        .expect("district");
        let building = scene_create_impl(
            &state,
            "Town Hall".to_owned(),
            ObjectKind::Building,
            "minecraft:overworld".to_owned(),
            BlockBox::new(BlockPos::new(4, 64, 4), BlockPos::new(6, 64, 4)).expect("bounds"),
            Some(district.id.clone()),
            BTreeMap::new(),
            None,
        )
        .expect("building");
        assert_eq!(building.id, "town-hall");
        assert_eq!(
            state.service.lock().expect("service").world_info().revision,
            revision_after_block,
            "scene metadata is not a world edit"
        );

        let inspection = {
            let service = state.service.lock().expect("service");
            match service
                .query(Query::InspectSceneObject {
                    object_id: building.id.clone(),
                })
                .expect("inspect")
            {
                QueryResult::SceneObject(inspection) => inspection,
                other => panic!("unexpected query result: {other:?}"),
            }
        };
        assert_eq!(inspection.object.name, "Town Hall");
        assert_eq!(inspection.volume, 3);
        assert_eq!(inspection.non_air_blocks, 3);
        assert_eq!(
            inspection
                .palette
                .iter()
                .find(|entry| entry.state == "minecraft:stone")
                .map(|entry| entry.count),
            Some(1)
        );

        let reopened = state
            .projects
            .lock()
            .expect("projects")
            .store
            .open(&folder)
            .expect("reopen");
        assert_eq!(reopened.0.editor_state.scene.objects().len(), 2);
        assert_eq!(
            reopened.0.editor_state.scene.objects()[0].tags["style"],
            "medieval"
        );
        assert_eq!(
            reopened.0.editor_state.scene.objects()[1]
                .parent_id
                .as_deref(),
            Some("old-town")
        );

        let error = scene_delete_impl(&state, "old-town").expect_err("children");
        assert_eq!(error.code, ErrorCode::Conflict);
        scene_delete_impl(&state, "town-hall").expect("delete child");
        scene_delete_impl(&state, "old-town").expect("delete parent");
        let reopened = state
            .projects
            .lock()
            .expect("projects")
            .store
            .open(&folder)
            .expect("reopen");
        assert!(reopened.0.editor_state.scene.is_empty());

        let error = scene_create_impl(
            &state,
            "Bad".to_owned(),
            ObjectKind::Other,
            "minecraft:moon".to_owned(),
            BlockBox::single(BlockPos::new(0, 64, 0)),
            None,
            BTreeMap::new(),
            None,
        )
        .expect_err("unknown dimension");
        assert_eq!(error.code, ErrorCode::NotFound);
        assert_eq!(
            state.service.lock().expect("service").world_info().revision,
            revision_after_block
        );
    }

    #[test]
    fn parametric_recipes_persist_and_resolve_manual_conflicts() {
        use world_application::{
            GeneratorKind, HouseParams, ParametricConflictPolicy, ParametricSpec, Rotation,
            HOUSE_GENERATOR_VERSION,
        };

        let (state, folder) = project_state("parametric");
        let project_id = current_project(&state).expect("current").project_id;
        {
            let mut service = state.service.lock().expect("service");
            service
                .execute(OperationRequest {
                    project_id: project_id.clone(),
                    operation: Operation::SetBlock {
                        dimension: "minecraft:overworld".to_owned(),
                        pos: BlockPos::new(14, 65, 14),
                        state: "minecraft:gold_block".parse().expect("state"),
                    },
                    expected_revision: None,
                    idempotency_key: None,
                    actor: ui_actor(),
                    linked_object_id: None,
                    layer_id: None,
                })
                .expect("gold");
        }
        let revision = state.service.lock().expect("service").world_info().revision;
        let spec = ParametricSpec {
            name: "Parametric House".to_owned(),
            generator: GeneratorKind::House,
            generator_version: HOUSE_GENERATOR_VERSION,
            dimension: "minecraft:overworld".to_owned(),
            origin: BlockPos::new(14, 64, 14),
            rotation: Rotation::None,
            params: HouseParams {
                width: 5,
                depth: 4,
                height: 3,
                wall: "minecraft:stone_bricks".to_owned(),
                floor: "minecraft:oak_planks".to_owned(),
                roof: "minecraft:dark_oak_planks".to_owned(),
                window: Some("minecraft:glass".to_owned()),
            },
            layer_id: None,
        };
        let created = parametric_create_impl(&state, spec.clone()).expect("create");
        assert_eq!(created.object.id, "parametric-house");
        assert!(created.operation.is_some());
        assert!(
            state.service.lock().expect("service").world_info().revision > revision,
            "the voxel overlay is a world edit"
        );
        assert_eq!(
            state
                .service
                .lock()
                .expect("service")
                .world()
                .get_block("minecraft:overworld", BlockPos::new(14, 65, 14))
                .expect("block")
                .to_string(),
            "minecraft:stone_bricks"
        );

        let reopened = state
            .projects
            .lock()
            .expect("projects")
            .store
            .open(&folder)
            .expect("reopen");
        assert_eq!(reopened.0.editor_state.parametric.objects().len(), 1);
        assert_eq!(
            reopened.0.editor_state.parametric.objects()[0]
                .baseline
                .len(),
            created.object.generated_blocks as usize
        );
        assert_eq!(
            reopened.0.editor_state.parametric.objects()[0].applied_revision,
            created
                .operation
                .as_ref()
                .expect("operation")
                .world_revision
        );

        {
            let mut service = state.service.lock().expect("service");
            service
                .execute(OperationRequest {
                    project_id: project_id.clone(),
                    operation: Operation::SetBlock {
                        dimension: "minecraft:overworld".to_owned(),
                        pos: BlockPos::new(14, 65, 14),
                        state: "minecraft:glass".parse().expect("state"),
                    },
                    expected_revision: None,
                    idempotency_key: None,
                    actor: ui_actor(),
                    linked_object_id: None,
                    layer_id: None,
                })
                .expect("manual edit");
        }
        let revision = state.service.lock().expect("service").world_info().revision;
        let mut taller = spec.clone();
        taller.params.height = 4;
        let error = parametric_update_impl(
            &state,
            "parametric-house",
            taller.clone(),
            ParametricConflictPolicy::Fail,
        )
        .expect_err("manual conflict");
        assert_eq!(error.code, ErrorCode::Conflict);
        assert_eq!(
            state.service.lock().expect("service").world_info().revision,
            revision
        );
        let conflict = parametric_inspect_impl(&state, "parametric-house").expect("inspect");
        assert!(conflict.conflict_count >= 1);

        let updated = parametric_update_impl(
            &state,
            "parametric-house",
            taller,
            ParametricConflictPolicy::Override,
        )
        .expect("override");
        assert!(!updated.conflicts.is_empty());
        assert_eq!(
            state
                .service
                .lock()
                .expect("service")
                .world()
                .get_block("minecraft:overworld", BlockPos::new(14, 65, 14))
                .expect("block")
                .to_string(),
            "minecraft:stone_bricks"
        );
        let reopened = state
            .projects
            .lock()
            .expect("projects")
            .store
            .open(&folder)
            .expect("reopen");
        assert_eq!(
            reopened.0.editor_state.parametric.objects()[0]
                .params
                .height,
            4
        );

        let baked = parametric_bake_impl(&state, "parametric-house").expect("bake");
        assert!(baked.operation.is_none());
        let reopened = state
            .projects
            .lock()
            .expect("projects")
            .store
            .open(&folder)
            .expect("reopen");
        assert!(reopened.0.editor_state.parametric.is_empty());
        assert_eq!(
            state
                .service
                .lock()
                .expect("service")
                .world()
                .get_block("minecraft:overworld", BlockPos::new(14, 64, 14))
                .expect("block")
                .to_string(),
            "minecraft:oak_planks",
            "bake keeps the voxel result"
        );
    }
}

#[cfg(test)]
mod component_command_tests {
    use super::*;

    #[test]
    fn component_errors_map_to_stable_app_codes() {
        assert_eq!(
            component_error(ComponentError::InvalidRequest("bad".to_owned())).code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            component_error(ComponentError::Runtime(
                runtime_manager::RuntimeError::Offline
            ))
            .code,
            ErrorCode::Offline
        );
        assert_eq!(
            component_error(ComponentError::Runtime(
                runtime_manager::RuntimeError::DiskSpace("full".to_owned())
            ))
            .code,
            ErrorCode::DiskSpace
        );
        assert_eq!(
            component_error(ComponentError::ClientJar(
                component_manager::ClientJarError::Offline
            ))
            .code,
            ErrorCode::Offline
        );
        assert_eq!(
            component_error(ComponentError::Busy { kind: "assets" }).code,
            ErrorCode::Conflict
        );
    }

    #[test]
    fn component_events_serialize_with_camel_case_and_stable_kinds() {
        let changed = serde_json::to_value(ComponentChangedEvent {
            kind: ComponentKind::Java,
        })
        .expect("changed");
        assert_eq!(changed["kind"], "java");

        let failure = serde_json::to_value(ComponentErrorEvent {
            kind: ComponentKind::Assets,
            code: "offline".to_owned(),
            message: "network access is disabled".to_owned(),
        })
        .expect("failure");
        assert_eq!(failure["kind"], "assets");
        assert_eq!(failure["code"], "offline");
    }
}
