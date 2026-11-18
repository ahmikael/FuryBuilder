//! Local authenticated IPC endpoint for the MCP bridge.
//!
//! The endpoint is a local socket (a Windows named pipe on the first target platform)
//! created by the running editor. A user-private discovery file publishes the endpoint
//! name and a random per-launch token; the bridge never opens project or world files.
//! Every request is handled by the same `WorldService` used by the UI, so world data,
//! revisions and policy stay authoritative in the Core.

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericNamespaced, Listener, ListenerOptions, Stream};
use interprocess::TryClone;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;
use world_application::{
    app_info, apply_job_operation, undo_job_operation_id, Actor, ActorSource, AppError, ErrorCode,
    JobStartRequest, JobState, Operation, OperationRequest, OperationResult, PreviewResult, Query,
    UndoTarget, WorldService, DEFAULT_JOB_LIST_LIMIT, MAX_JOB_LIST_LIMIT,
};

use crate::mcp_access::{McpAccessInput, McpAccessMode};
use crate::mcp_activity::{
    clean_activity_text, McpActivityEntry, McpActivityStatus, DEFAULT_MCP_ACTIVITY_QUERY,
    MAX_MCP_ACTIVITY_QUERY,
};
use crate::state::{session_status, AppState, LOCAL_PROJECT_ID};

pub const PROTOCOL_VERSION: u32 = 1;
pub const DISCOVERY_SCHEMA_VERSION: u32 = 1;
pub const RENDERER_REQUEST_EVENT: &str = "furybuilder-renderer-request";
pub const MCP_CONFIRMATION_EVENT: &str = "furybuilder-mcp-confirmation-request";
/// Tells the mounted editor that an MCP write committed. The payload carries the same
/// revision and dirty chunks as a UI write so the viewport is not left on a stale frame.
pub const WORLD_CHANGED_EVENT: &str = "furybuilder-world-changed";

const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_REQUEST_ID_BYTES: usize = 128;
const DISCOVERY_DIR: &str = "mcp";
const DISCOVERY_FILE: &str = "runtime.json";
const MAX_CAPTURE_DATA_URL_BYTES: usize = 12 * 1024 * 1024;
const MAX_RENDERER_RESPONSE_BYTES: usize = MAX_CAPTURE_DATA_URL_BYTES + 256 * 1024;
const MAX_PENDING_RENDERER_REQUESTS: usize = 8;
const RENDERER_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const MCP_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(90);
const PNG_DATA_URL_PREFIX: &str = "data:image/png;base64,";

/// Pending viewport requests. The Tauri UI answers them through the
/// `submit_renderer_response` command; a request that times out is removed so a late
/// answer is rejected instead of reaching a different caller.
#[derive(Debug, Default)]
pub struct RendererBridge {
    pending: std::sync::Mutex<HashMap<String, mpsc::Sender<Value>>>,
    ready_at_ms: std::sync::Mutex<Option<u64>>,
}

impl RendererBridge {
    pub fn mark_ready(&self) {
        if let Ok(mut ready) = self.ready_at_ms.lock() {
            *ready = Some(now_ms());
        }
    }

    fn readiness(&self) -> Value {
        match self.ready_at_ms.lock().ok().and_then(|ready| *ready) {
            Some(ready_at_ms) => json!({ "listening": true, "readyAtMs": ready_at_ms }),
            None => json!({ "listening": false, "readyAtMs": null }),
        }
    }

    fn open(&self, request_id: &str) -> Result<mpsc::Receiver<Value>, AppError> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| AppError::internal("renderer bridge lock poisoned"))?;
        if pending.len() >= MAX_PENDING_RENDERER_REQUESTS {
            return Err(AppError::new(
                ErrorCode::LimitExceeded,
                "too many viewport requests are already pending",
            ));
        }
        let (sender, receiver) = mpsc::channel();
        pending.insert(request_id.to_owned(), sender);
        Ok(receiver)
    }

    fn complete(&self, request_id: &str, response: Value) -> bool {
        let sender = self
            .pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(request_id));
        sender.is_some_and(|sender| sender.send(response).is_ok())
    }

    fn cancel(&self, request_id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(request_id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererRequestKind {
    Camera,
    Capture,
}

impl RendererRequestKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Camera => "camera",
            Self::Capture => "capture",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RendererRequest {
    pub kind: RendererRequestKind,
    pub project_id: String,
    pub dimension: Option<String>,
}

/// Sends a read-only viewport request and waits for the Tauri UI to answer it. The
/// application tests substitute a fake so capture handling can be verified without a window.
pub trait RendererRequester: Sync {
    fn request(&self, state: &AppState, request: RendererRequest) -> Result<Value, AppError>;

    /// Asks the editor UI to approve a write. The default is a fail-closed answer for
    /// tests and headless runs; the Tauri implementation emits a real confirmation event.
    fn confirm(&self, _state: &AppState, _summary: Value) -> Result<bool, AppError> {
        Err(AppError::new(
            ErrorCode::BackendUnavailable,
            "the editor confirmation UI is not attached; open FuryBuilder and retry",
        ))
    }

    /// Notifies a mounted editor that the authoritative world advanced. Tests and headless
    /// runs have no viewport, so the default is deliberately a no-op.
    fn world_changed(&self, _payload: Value) {}
}

/// Used when the local endpoint runs without a connected viewport (tests and headless
/// restarts). It never returns fabricated camera or image data.
#[cfg(test)]
pub struct UnavailableRenderer;

#[cfg(test)]
impl RendererRequester for UnavailableRenderer {
    fn request(&self, _state: &AppState, _request: RendererRequest) -> Result<Value, AppError> {
        Err(AppError::new(
            ErrorCode::BackendUnavailable,
            "the editor viewport is not attached; open the FuryBuilder window and retry",
        ))
    }
}

struct TauriRendererRequester {
    app: AppHandle,
}

impl RendererRequester for TauriRendererRequester {
    fn request(&self, state: &AppState, request: RendererRequest) -> Result<Value, AppError> {
        let request_id = Uuid::new_v4().simple().to_string();
        let receiver = state.renderer.open(&request_id)?;
        let payload = json!({
            "requestId": request_id,
            "kind": request.kind.as_str(),
            "projectId": request.project_id,
            "dimension": request.dimension,
        });
        if let Err(error) = self.app.emit(RENDERER_REQUEST_EVENT, payload) {
            state.renderer.cancel(&request_id);
            return Err(AppError::new(
                ErrorCode::BackendUnavailable,
                format!("the editor viewport is not available: {error}"),
            ));
        }
        let response = match receiver.recv_timeout(RENDERER_REQUEST_TIMEOUT) {
            Ok(response) => response,
            Err(_) => {
                state.renderer.cancel(&request_id);
                return Err(AppError::new(
                    ErrorCode::BackendUnavailable,
                    "the editor viewport did not answer the request in time",
                ));
            }
        };
        Ok(response)
    }

    fn confirm(&self, state: &AppState, summary: Value) -> Result<bool, AppError> {
        let request_id = Uuid::new_v4().simple().to_string();
        let expires_at_ms = now_ms() + MCP_CONFIRMATION_TIMEOUT.as_millis() as u64;
        let receiver = state
            .mcp_confirmations
            .open(&request_id, summary.clone(), expires_at_ms)?;
        let payload = json!({
            "requestId": request_id,
            "summary": summary,
            "expiresAtMs": expires_at_ms,
        });
        if let Err(error) = self.app.emit(MCP_CONFIRMATION_EVENT, payload) {
            state.mcp_confirmations.cancel(&request_id);
            return Err(AppError::new(
                ErrorCode::BackendUnavailable,
                format!("the editor confirmation UI is not available: {error}"),
            ));
        }
        match receiver.recv_timeout(MCP_CONFIRMATION_TIMEOUT) {
            Ok(approved) => Ok(approved),
            Err(_) => {
                state.mcp_confirmations.cancel(&request_id);
                Err(AppError::new(
                    ErrorCode::PermissionDenied,
                    "MCP write confirmation timed out; nothing was changed",
                )
                .with_details(json!({ "confirmation": "timeout" })))
            }
        }
    }

    fn world_changed(&self, payload: Value) {
        let _ = self.app.emit(WORLD_CHANGED_EVENT, payload);
    }
}

/// Payload shared by synchronous MCP writes and completed MCP jobs. The UI treats it as
/// an external write: refresh the summary/history and rebuild the named chunks.
fn world_change_payload(
    action: &str,
    project_id: &str,
    job_id: Option<&str>,
    result: &OperationResult,
) -> Value {
    json!({
        "source": "mcp",
        "action": action,
        "projectId": project_id,
        "jobId": job_id,
        "operationId": result.operation_id,
        "worldRevision": result.world_revision,
        "changedBlocks": result.changed_blocks,
        "dirtyChunks": result.dirty_chunks,
        "reload": false
    })
}

/// The file the bridge reads to discover the endpoint. It contains a secret token and is
/// therefore written to the application data directory, never to logs or tool output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeRuntimeFile {
    pub schema_version: u32,
    pub transport: String,
    pub endpoint: String,
    pub token: String,
    pub pid: u32,
    pub protocol_version: u32,
    pub app_version: String,
    pub started_at_ms: u64,
}

/// Managed state that keeps the discovery file alive for the lifetime of the process and
/// removes it on a clean exit.
pub struct LocalBridgeRuntime {
    discovery_path: PathBuf,
}

impl LocalBridgeRuntime {
    pub fn cleanup(&self) {
        let _ = fs::remove_file(&self.discovery_path);
    }
}

impl Drop for LocalBridgeRuntime {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireRequest {
    protocol: u32,
    id: String,
    token: String,
    method: String,
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireResponse {
    protocol: u32,
    id: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<AppError>,
}

impl WireResponse {
    fn ok(id: String, result: Value) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: String, error: AppError) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            id,
            ok: false,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProjectParams {
    project_id: String,
    #[serde(default)]
    world_revision: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryParams {
    project_id: String,
    #[serde(default)]
    world_revision: Option<u64>,
    query: Query,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RenderParams {
    project_id: String,
    #[serde(default)]
    world_revision: Option<u64>,
    #[serde(default)]
    dimension: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OperationParams {
    project_id: String,
    operation: Operation,
    #[serde(default)]
    world_revision: Option<u64>,
    #[serde(default)]
    idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct UndoParams {
    project_id: String,
    #[serde(default)]
    world_revision: Option<u64>,
    #[serde(default)]
    operation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StatusParams {
    project_id: String,
    #[serde(default)]
    operation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ActivityParams {
    project_id: String,
    #[serde(default)]
    operation_id: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StartJobParams {
    project_id: String,
    kind: String,
    #[serde(default)]
    parameters: Value,
    idempotency_key: String,
    #[serde(default)]
    world_revision: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JobParams {
    project_id: String,
    job_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JobListParams {
    project_id: String,
    #[serde(default)]
    limit: Option<usize>,
}

/// Starts the local endpoint. Failure is reported to the caller instead of panicking so a
/// missing pipe cannot stop the editor from opening; the UI remains fully usable.
pub fn start(app: AppHandle, data_dir: std::path::PathBuf) -> io::Result<LocalBridgeRuntime> {
    let discovery_dir = data_dir.join(DISCOVERY_DIR);
    fs::create_dir_all(&discovery_dir)?;
    let discovery_path = discovery_dir.join(DISCOVERY_FILE);
    let _ = fs::remove_file(&discovery_path);

    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let suffix = Uuid::new_v4().simple().to_string();
    let endpoint_name = format!("furybuilder-mcp-{}-{}", std::process::id(), &suffix[..12]);
    let name = endpoint_name.clone().to_ns_name::<GenericNamespaced>()?;
    let listener = ListenerOptions::new().name(name).create_sync()?;

    let runtime = BridgeRuntimeFile {
        schema_version: DISCOVERY_SCHEMA_VERSION,
        transport: "local_socket".to_owned(),
        endpoint: endpoint_display_path(&endpoint_name),
        token: token.clone(),
        pid: std::process::id(),
        protocol_version: PROTOCOL_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        started_at_ms: now_ms(),
    };
    write_discovery_file(&discovery_path, &runtime)?;

    // Completed MCP jobs commit outside the UI command path, so tell the mounted editor
    // to rebuild the affected chunks. Failed and cancelled jobs wrote nothing.
    {
        let event_app = app.clone();
        let state = app.state::<AppState>();
        state.jobs.set_listener(move |snapshot| {
            if snapshot.state != JobState::Succeeded {
                return;
            }
            let Some(result) = &snapshot.result else {
                return;
            };
            let action = if snapshot.kind == "undo_operation" {
                "undo_job"
            } else {
                "apply_job"
            };
            let _ = event_app.emit(
                WORLD_CHANGED_EVENT,
                world_change_payload(action, &snapshot.project_id, Some(&snapshot.job_id), result),
            );
        });
    }

    let spawned = std::thread::Builder::new()
        .name("furybuilder-mcp-listener".to_owned())
        .spawn(move || run_listener(listener, token, app));
    if let Err(error) = spawned {
        let _ = fs::remove_file(&discovery_path);
        return Err(error);
    }

    Ok(LocalBridgeRuntime { discovery_path })
}

fn run_listener(listener: Listener, token: String, app: AppHandle) {
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                let token = token.clone();
                let app = app.clone();
                let spawn = std::thread::Builder::new()
                    .name("furybuilder-mcp-request".to_owned())
                    .spawn(move || {
                        let state = app.state::<AppState>();
                        let requester = TauriRendererRequester { app: app.clone() };
                        if let Err(error) = serve_connection(stream, &token, &state, &requester) {
                            eprintln!("FuryBuilder MCP bridge request failed: {error}");
                        }
                    });
                if let Err(error) = spawn {
                    eprintln!("FuryBuilder MCP bridge could not spawn a request: {error}");
                }
            }
            Err(error) => eprintln!("FuryBuilder MCP bridge listener error: {error}"),
        }
    }
}

fn serve_connection(
    stream: Stream,
    expected_token: &str,
    state: &AppState,
    requester: &dyn RendererRequester,
) -> io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;

    let mut raw = Vec::new();
    reader
        .by_ref()
        .take((MAX_MESSAGE_BYTES + 1) as u64)
        .read_until(b'\n', &mut raw)?;
    if raw.is_empty() {
        return Ok(());
    }

    let response = if raw.len() > MAX_MESSAGE_BYTES {
        WireResponse::error(
            "unknown".to_owned(),
            AppError::invalid_argument("bridge request exceeds the message size limit"),
        )
    } else {
        match std::str::from_utf8(&raw)
            .ok()
            .and_then(|text| serde_json::from_str::<WireRequest>(text).ok())
        {
            Some(request) => {
                handle_request_with_renderer(state, expected_token, request, requester)
            }
            None => WireResponse::error(
                "unknown".to_owned(),
                AppError::invalid_argument("bridge request is not valid protocol JSON"),
            ),
        }
    };

    let encoded = serde_json::to_vec(&response)
        .map_err(|error| io::Error::other(format!("serializing bridge response: {error}")))?;
    writer.write_all(&encoded)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

#[cfg(test)]
fn handle_request(state: &AppState, expected_token: &str, request: WireRequest) -> WireResponse {
    handle_request_with_renderer(state, expected_token, request, &UnavailableRenderer)
}

fn handle_request_with_renderer(
    state: &AppState,
    expected_token: &str,
    request: WireRequest,
    requester: &dyn RendererRequester,
) -> WireResponse {
    let id = request.id;
    if request.protocol != PROTOCOL_VERSION {
        return WireResponse::error(
            id,
            AppError::new(
                ErrorCode::UnsupportedVersion,
                format!(
                    "bridge protocol {} is not supported; expected {PROTOCOL_VERSION}",
                    request.protocol
                ),
            ),
        );
    }
    if id.trim().is_empty() || id.len() > MAX_REQUEST_ID_BYTES {
        return WireResponse::error(
            id,
            AppError::invalid_argument("bridge request id must be 1..=128 characters"),
        );
    }
    if !token_matches(expected_token, &request.token) {
        return WireResponse::error(
            id,
            AppError::new(
                ErrorCode::Unauthorized,
                "bridge authentication failed; restart the editor and its MCP client",
            ),
        );
    }

    let method = request.method.clone();
    match dispatch(
        state,
        &id,
        request.client_id.as_deref(),
        &method,
        request.params,
        requester,
    ) {
        Ok(result) => WireResponse::ok(id, result),
        Err(error) => WireResponse::error(id, error),
    }
}

/// Records one authenticated request and then delegates to the authoritative handler.
/// `ping` and the activity query itself are excluded so health checks cannot flood the
/// journal.
fn dispatch(
    state: &AppState,
    request_id: &str,
    client_id: Option<&str>,
    method: &str,
    params: Value,
    requester: &dyn RendererRequester,
) -> Result<Value, AppError> {
    let activity = ActivityMeta::new(request_id, client_id, method, &params);
    let outcome = dispatch_inner(state, method, params, requester);
    if method != "ping" && method != "mcp_activity" {
        state.mcp_activity.record(activity.finish(&outcome));
    }
    outcome
}

fn dispatch_inner(
    state: &AppState,
    method: &str,
    params: Value,
    requester: &dyn RendererRequester,
) -> Result<Value, AppError> {
    match method {
        "ping" => {
            let (project_id, project_name) = active_project_identity(state)?;
            let revision = world_revision(state)?;
            Ok(json!({
                "app": app_info(),
                "project": {
                    "projectId": project_id,
                    "name": project_name,
                    "worldRevision": revision,
                },
                "bridge": {
                    "protocolVersion": PROTOCOL_VERSION,
                    "transport": "local_socket",
                    "pid": std::process::id(),
                },
                "renderer": state.renderer.readiness(),
                "mcp": state.mcp_access.summary(),
                "jobs": state.jobs.summary(),
                "pendingConfirmations": state.mcp_confirmations.count(),
            }))
        }
        "world_info" => {
            let params: ProjectParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            require_revision(state, params.world_revision)?;
            let service = state
                .service
                .lock()
                .map_err(|_| AppError::internal("world state lock poisoned"))?;
            to_value(service.world_info())
        }
        "query" => {
            let params: QueryParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            require_revision(state, params.world_revision)?;
            let service = state
                .service
                .lock()
                .map_err(|_| AppError::internal("world state lock poisoned"))?;
            let revision = service.world().revision();
            let mut value = to_value(service.query(params.query)?)?;
            // The read and the revision are taken under one lock. Publishing it here means
            // a block or region observation is always attributable to an exact revision
            // instead of leaving the client to guess from an earlier call.
            value["worldRevision"] = json!(revision);
            Ok(value)
        }
        "project_status" => {
            let params: ProjectParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            require_revision(state, params.world_revision)?;
            to_value(session_status(state)?)
        }
        "get_camera" => {
            let params: RenderParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            require_revision(state, params.world_revision)?;
            request_renderer(state, requester, RendererRequestKind::Camera, params)
        }
        "capture_viewport" => {
            let params: RenderParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            require_revision(state, params.world_revision)?;
            request_renderer(state, requester, RendererRequestKind::Capture, params)
        }
        "preview_operation" => {
            let params: OperationParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            require_revision(state, params.world_revision)?;
            let service = state
                .service
                .lock()
                .map_err(|_| AppError::internal("world state lock poisoned"))?;
            let preview = service.preview(&params.project_id, &params.operation)?;
            let mut value = to_value(preview)?;
            value["worldRevision"] = json!(service.world().revision());
            value["access"] = match state.mcp_access.authorize_operation(&params.operation) {
                Ok(()) => json!({ "allowed": true }),
                Err(error) => json!({
                    "allowed": false,
                    "code": error.code,
                    "message": error.message,
                    "details": error.details,
                }),
            };
            Ok(value)
        }
        "apply_operation" => {
            let params: OperationParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            let revision = require_write_revision(params.world_revision)?;
            state.mcp_access.authorize_operation(&params.operation)?;
            require_revision(state, Some(revision))?;
            let preview = {
                let service = state
                    .service
                    .lock()
                    .map_err(|_| AppError::internal("world state lock poisoned"))?;
                service.preview(&params.project_id, &params.operation)?
            };
            let mut confirmed = false;
            if state.mcp_access.require_confirmation() {
                let summary = apply_confirmation_summary(
                    &params,
                    revision,
                    &preview,
                    state.mcp_access.mode(),
                );
                confirm_write(state, requester, summary)?;
                confirmed = true;
                // The user may have changed the policy while the dialog was open.
                state.mcp_access.authorize_operation(&params.operation)?;
                require_revision(state, Some(revision))?;
            }
            let project_id = params.project_id.clone();
            let (mut value, change) = {
                let mut service = state
                    .service
                    .lock()
                    .map_err(|_| AppError::internal("world state lock poisoned"))?;
                let result = service.execute(OperationRequest {
                    project_id: project_id.clone(),
                    operation: params.operation,
                    expected_revision: Some(revision),
                    idempotency_key: params.idempotency_key,
                    actor: Actor::mcp("mcp-bridge"),
                    linked_object_id: None,
                })?;
                let change = world_change_payload("apply", &project_id, None, &result);
                (to_value(result)?, change)
            };
            if confirmed {
                value["confirmation"] = json!({ "approved": true });
            }
            requester.world_changed(change);
            Ok(value)
        }
        "undo_operation" => {
            let params: UndoParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            let revision = require_write_revision(params.world_revision)?;
            require_revision(state, Some(revision))?;
            state.mcp_access.require_editing("undo_operation")?;
            let (target, summary) = {
                let service = state
                    .service
                    .lock()
                    .map_err(|_| AppError::internal("world state lock poisoned"))?;
                let target = service.undo_target(params.operation_id.as_deref())?;
                state
                    .mcp_access
                    .authorize_undo(&target.dimension, target.bounds)?;
                let summary =
                    undo_confirmation_summary(&params, revision, &target, state.mcp_access.mode());
                (target, summary)
            };
            let mut confirmed = false;
            if state.mcp_access.require_confirmation() {
                confirm_write(state, requester, summary)?;
                confirmed = true;
                state
                    .mcp_access
                    .authorize_undo(&target.dimension, target.bounds)?;
                require_revision(state, Some(revision))?;
            }
            let project_id = params.project_id.clone();
            let (mut value, change) = {
                let mut service = state
                    .service
                    .lock()
                    .map_err(|_| AppError::internal("world state lock poisoned"))?;
                let result = service.undo(
                    &project_id,
                    Actor::mcp("mcp-bridge"),
                    Some(target.operation_id.as_str()),
                )?;
                let change = world_change_payload("undo", &project_id, None, &result);
                (to_value(result)?, change)
            };
            if confirmed {
                value["confirmation"] = json!({ "approved": true });
            }
            requester.world_changed(change);
            Ok(value)
        }
        "start_job" => {
            let params: StartJobParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            start_job(state, requester, params)
        }
        "job_status" => {
            let params: JobParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            to_value(state.jobs.status(&params.project_id, &params.job_id)?)
        }
        "cancel_job" => {
            let params: JobParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            to_value(state.jobs.cancel(&params.project_id, &params.job_id)?)
        }
        "list_jobs" => {
            let params: JobListParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            let limit = params.limit.unwrap_or(DEFAULT_JOB_LIST_LIMIT);
            if !(1..=MAX_JOB_LIST_LIMIT).contains(&limit) {
                return Err(AppError::invalid_argument(format!(
                    "job limit must be between 1 and {MAX_JOB_LIST_LIMIT}"
                )));
            }
            Ok(json!({
                "projectId": params.project_id,
                "jobs": state.jobs.list(&params.project_id, limit),
                "limit": limit,
                "summary": state.jobs.summary(),
            }))
        }
        "operation_status" => {
            let params: StatusParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            let service = state
                .service
                .lock()
                .map_err(|_| AppError::internal("world state lock poisoned"))?;
            operation_status(
                state,
                &service,
                &params.project_id,
                params.operation_id.as_deref(),
            )
        }
        "mcp_activity" => {
            let params: ActivityParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            activity_response(
                state,
                &params.project_id,
                params.operation_id.as_deref(),
                params.limit,
            )
        }
        other => Err(AppError::invalid_argument(format!(
            "unknown bridge method `{other}`"
        ))),
    }
}

fn start_job(
    state: &AppState,
    requester: &dyn RendererRequester,
    params: StartJobParams,
) -> Result<Value, AppError> {
    let kind = params.kind.as_str();
    if !matches!(kind, "apply_operation" | "undo_operation") {
        return Err(AppError::invalid_argument(format!(
            "unsupported job kind `{}`; only apply_operation and undo_operation are exposed over MCP",
            params.kind
        ))
        .with_details(json!({
            "supportedKinds": ["apply_operation", "undo_operation"]
        })));
    }

    let revision = require_write_revision(params.world_revision)?;
    let request = JobStartRequest {
        project_id: params.project_id.clone(),
        kind: params.kind.clone(),
        parameters: params.parameters.clone(),
        idempotency_key: params.idempotency_key.clone(),
        expected_revision: Some(revision),
        actor: Actor::mcp("mcp-bridge"),
    };

    // A retried tool call returns the existing job without asking for approval again,
    // queueing a second world edit, or failing on the revision the job itself produced.
    // A different payload under the same key is still a conflict.
    if let Some(existing) = state.jobs.replay(&request)? {
        return to_value(existing);
    }
    require_revision(state, Some(revision))?;

    let confirmed = match kind {
        "apply_operation" => {
            let operation = apply_job_operation(&params.parameters)?;
            state.mcp_access.authorize_operation(&operation)?;
            let preview = {
                let service = state
                    .service
                    .lock()
                    .map_err(|_| AppError::internal("world state lock poisoned"))?;
                service.preview(&params.project_id, &operation)?
            };
            if state.mcp_access.require_confirmation() {
                let summary_params = OperationParams {
                    project_id: params.project_id.clone(),
                    operation,
                    world_revision: Some(revision),
                    idempotency_key: Some(params.idempotency_key.clone()),
                };
                confirm_write(
                    state,
                    requester,
                    apply_confirmation_summary(
                        &summary_params,
                        revision,
                        &preview,
                        state.mcp_access.mode(),
                    ),
                )?;
                state
                    .mcp_access
                    .authorize_operation(&summary_params.operation)?;
                require_revision(state, Some(revision))?;
                true
            } else {
                false
            }
        }
        "undo_operation" => {
            state.mcp_access.require_editing("undo_operation")?;
            let operation_id = undo_job_operation_id(&params.parameters)?;
            let summary = {
                let service = state
                    .service
                    .lock()
                    .map_err(|_| AppError::internal("world state lock poisoned"))?;
                let target = service.undo_target(operation_id.as_deref())?;
                state
                    .mcp_access
                    .authorize_undo(&target.dimension, target.bounds)?;
                undo_confirmation_summary(
                    &UndoParams {
                        project_id: params.project_id.clone(),
                        world_revision: Some(revision),
                        operation_id: operation_id.clone(),
                    },
                    revision,
                    &target,
                    state.mcp_access.mode(),
                )
            };
            if state.mcp_access.require_confirmation() {
                confirm_write(state, requester, summary)?;
                state.mcp_access.require_editing("undo_operation")?;
                require_revision(state, Some(revision))?;
                true
            } else {
                false
            }
        }
        _ => unreachable!("kind was validated above"),
    };

    let mut value = to_value(state.jobs.start(request)?)?;

    // The policy may have been narrowed after the approval while this request was being
    // queued. Cancel before returning the error; `set_mcp_access` also cancels active MCP
    // jobs, so an already-running job is rolled back instead of committing.
    let job_state = value.get("state").and_then(Value::as_str);
    if matches!(job_state, Some("queued" | "running")) {
        let recheck = if kind == "apply_operation" {
            apply_job_operation(&params.parameters)
                .and_then(|operation| state.mcp_access.authorize_operation(&operation))
        } else {
            state.mcp_access.require_editing("undo_operation")
        };
        if let Err(error) = recheck {
            if let Some(job_id) = value.get("jobId").and_then(Value::as_str) {
                let _ = state.jobs.cancel(&params.project_id, job_id);
            }
            return Err(error);
        }
    }

    if confirmed {
        value["confirmation"] = json!({ "approved": true, "deferred": true });
    }
    Ok(value)
}

fn require_write_revision(revision: Option<u64>) -> Result<u64, AppError> {
    revision.ok_or_else(|| {
        AppError::invalid_argument(
            "writes require world_revision so a stale edit cannot overwrite newer work",
        )
    })
}

fn confirm_write(
    state: &AppState,
    requester: &dyn RendererRequester,
    summary: Value,
) -> Result<(), AppError> {
    if requester.confirm(state, summary.clone())? {
        return Ok(());
    }
    Err(AppError::new(
        ErrorCode::PermissionDenied,
        "the user denied this MCP write; nothing was changed",
    )
    .with_details(json!({ "confirmation": "denied", "summary": summary })))
}

fn apply_confirmation_summary(
    params: &OperationParams,
    revision: u64,
    preview: &PreviewResult,
    mode: McpAccessMode,
) -> Value {
    json!({
        "kind": "apply",
        "projectId": params.project_id,
        "worldRevision": revision,
        "accessMode": mode,
        "operation": operation_summary(&params.operation),
        "affectedBlocks": preview.affected_blocks,
        "changedBlocks": preview.changed_blocks,
        "dirtyChunks": preview.dirty_chunks.len(),
        "warnings": preview.warnings,
        "idempotencyKey": params.idempotency_key,
    })
}

fn undo_confirmation_summary(
    params: &UndoParams,
    revision: u64,
    target: &UndoTarget,
    mode: McpAccessMode,
) -> Value {
    json!({
        "kind": "undo",
        "projectId": params.project_id,
        "worldRevision": revision,
        "accessMode": mode,
        "operationId": target.operation_id,
        "dimension": target.dimension,
        "bounds": target.bounds,
        "blocks": target.bounds.checked_volume(),
    })
}

fn operation_summary(operation: &Operation) -> Value {
    let dimension = operation.dimension();
    let bounds = operation.bounds();
    match operation {
        Operation::SetBlock { pos, state, .. } => json!({
            "type": "set_block",
            "dimension": dimension,
            "bounds": bounds,
            "pos": pos,
            "state": state.to_string(),
        }),
        Operation::Batch { changes, .. } => json!({
            "type": "batch",
            "dimension": dimension,
            "bounds": bounds,
            "changes": changes.len(),
            "sample": changes.iter().take(8).map(|change| json!({
                "pos": change.pos,
                "state": change.state.to_string(),
            })).collect::<Vec<_>>(),
            "sampleTruncated": changes.len() > 8,
        }),
        Operation::FillCuboid { state, .. } => json!({
            "type": "fill_cuboid",
            "dimension": dimension,
            "bounds": bounds,
            "state": state.to_string(),
        }),
        Operation::Replace { from, to, .. } => json!({
            "type": "replace",
            "dimension": dimension,
            "bounds": bounds,
            "from": from.to_string(),
            "to": to.to_string(),
        }),
        Operation::TerrainBrush {
            kind,
            material,
            amount,
            seed,
            step,
            target_y,
            ..
        } => json!({
            "type": "terrain_brush",
            "dimension": dimension,
            "bounds": bounds,
            "kind": kind.label(),
            "material": material.to_string(),
            "amount": amount,
            "seed": seed,
            "step": step,
            "targetY": target_y,
        }),
    }
}

/// Operations are synchronous, so their authoritative status lives in the audit log.
/// Jobs/progress are explicitly unavailable rather than simulated. The status also names
/// the MCP requests that produced the operation, so a tool call and a history entry share
/// the same `operation_id`.
fn operation_status(
    state: &AppState,
    service: &WorldService,
    project_id: &str,
    operation_id: Option<&str>,
) -> Result<Value, AppError> {
    let revision = service.world().revision();
    if let Some(id) = operation_id {
        let entry = service
            .audit_log()
            .iter()
            .rev()
            .find(|entry| entry.operation_id == id)
            .ok_or_else(|| {
                AppError::new(ErrorCode::NotFound, format!("unknown operation `{id}`"))
            })?;
        return Ok(json!({
            "operationId": id,
            "status": "completed",
            "found": true,
            "action": entry.action,
            "actorId": entry.actor_id,
            "source": entry.source,
            "revisionBefore": entry.revision_before,
            "revisionAfter": entry.revision_after,
            "changedBlocks": entry.changed_blocks,
            "timestampMs": entry.timestamp_ms,
            "parameterSummary": entry.parameter_summary,
            "currentRevision": revision,
            "canUndo": service.can_undo(),
            "canRedo": service.can_redo(),
            "mcpActivity": state.mcp_activity.linked_operation(project_id, id),
            "job": job_status_value(state, project_id, Some(id)),
        }));
    }

    let recent: Vec<_> = service.history().into_iter().rev().take(10).collect();
    Ok(json!({
        "status": "idle",
        "currentRevision": revision,
        "canUndo": service.can_undo(),
        "canRedo": service.can_redo(),
        "recentOperations": recent,
        "mcpActivity": state.mcp_activity.query(project_id, None, 10),
        "job": job_status_value(state, project_id, None),
    }))
}

/// Job metadata for `operation_status`. The authoritative operation history remains the
/// source of truth; a retained job record is an additional link, not a replacement.
fn job_status_value(state: &AppState, project_id: &str, operation_id: Option<&str>) -> Value {
    let operation_job = operation_id.and_then(|id| state.jobs.find_by_operation(project_id, id));
    json!({
        "available": true,
        "summary": state.jobs.summary(),
        "recent": state.jobs.list(project_id, 5),
        "operationJob": operation_job,
    })
}

fn activity_response(
    state: &AppState,
    project_id: &str,
    operation_id: Option<&str>,
    limit: Option<usize>,
) -> Result<Value, AppError> {
    let limit = limit.unwrap_or(DEFAULT_MCP_ACTIVITY_QUERY);
    if !(1..=MAX_MCP_ACTIVITY_QUERY).contains(&limit) {
        return Err(AppError::invalid_argument(format!(
            "activity limit must be between 1 and {MAX_MCP_ACTIVITY_QUERY}"
        )));
    }
    if let Some(id) = operation_id {
        if id.trim().is_empty() || id.len() > MAX_REQUEST_ID_BYTES {
            return Err(AppError::invalid_argument(
                "operation id must be 1..=128 characters",
            ));
        }
    }
    let entries = state.mcp_activity.query(project_id, operation_id, limit);
    Ok(json!({
        "projectId": project_id,
        "entries": entries,
        "limit": limit,
        "stored": state.mcp_activity.len(),
        "bounded": true,
    }))
}

fn request_renderer(
    state: &AppState,
    requester: &dyn RendererRequester,
    kind: RendererRequestKind,
    params: RenderParams,
) -> Result<Value, AppError> {
    if let Some(dimension) = &params.dimension {
        if dimension.trim().is_empty() || dimension.len() > MAX_REQUEST_ID_BYTES {
            return Err(AppError::invalid_argument(
                "dimension must be 1..=128 characters",
            ));
        }
    }
    let requested_dimension = params.dimension.clone();
    let response = requester.request(
        state,
        RendererRequest {
            kind,
            project_id: params.project_id,
            dimension: params.dimension,
        },
    )?;
    let authoritative = world_revision(state)?;
    finalize_renderer_response(
        kind,
        requested_dimension.as_deref(),
        response,
        authoritative,
    )
}

/// Converts a UI answer into a result that never claims a newer revision than the
/// renderer actually produced. `stale` is true when the authoritative world moved on.
fn finalize_renderer_response(
    kind: RendererRequestKind,
    requested_dimension: Option<&str>,
    response: Value,
    authoritative_revision: u64,
) -> Result<Value, AppError> {
    if response.get("kind").and_then(Value::as_str) == Some("error") {
        let code = response
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("renderer_error");
        let message = response
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("the editor viewport rejected the request");
        return Err(AppError::new(ErrorCode::InvalidArgument, message)
            .with_details(json!({ "rendererCode": code })));
    }
    if response.get("kind").and_then(Value::as_str) != Some(kind.as_str()) {
        return Err(AppError::invalid_argument(
            "editor viewport returned an unexpected response kind",
        ));
    }
    let dimension = response
        .get("dimension")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::invalid_argument("editor viewport response has no dimension"))?;
    if let Some(requested) = requested_dimension {
        if requested != dimension {
            return Err(AppError::new(
                ErrorCode::NotFound,
                format!(
                    "requested dimension `{requested}` is not the dimension shown in the editor (`{dimension}`)"
                ),
            )
            .with_details(json!({
                "requestedDimension": requested,
                "actualDimension": dimension,
            })));
        }
    }
    let render_revision = response
        .get("worldRevision")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            AppError::invalid_argument("editor viewport response has no world revision")
        })?;

    match kind {
        RendererRequestKind::Camera => {
            let pose = response.get("pose").ok_or_else(|| {
                AppError::invalid_argument("editor viewport response has no camera pose")
            })?;
            validate_camera_pose(pose)?;
            Ok(json!({
                "kind": "camera",
                "dimension": dimension,
                "pose": pose,
                "capturedAtMs": response.get("capturedAtMs").cloned().unwrap_or(Value::Null),
                "verification": {
                    "authoritativeRevision": authoritative_revision,
                    "renderRevision": render_revision,
                    "stale": render_revision != authoritative_revision,
                },
            }))
        }
        RendererRequestKind::Capture => {
            let data_url = response
                .get("dataUrl")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AppError::invalid_argument("editor viewport response has no image")
                })?;
            if !data_url.starts_with(PNG_DATA_URL_PREFIX) {
                return Err(AppError::invalid_argument(
                    "editor viewport image must be a PNG data URL",
                ));
            }
            if data_url.len() > MAX_CAPTURE_DATA_URL_BYTES {
                return Err(AppError::new(
                    ErrorCode::LimitExceeded,
                    "viewport capture exceeds the image size limit",
                ));
            }
            let metadata = response.get("metadata").ok_or_else(|| {
                AppError::invalid_argument("editor viewport response has no capture metadata")
            })?;
            if metadata.get("dimension").and_then(Value::as_str) != Some(dimension) {
                return Err(AppError::invalid_argument(
                    "capture metadata dimension does not match the image",
                ));
            }
            let partial = metadata
                .get("partial")
                .and_then(Value::as_bool)
                .ok_or_else(|| {
                    AppError::invalid_argument("capture metadata has no partial flag")
                })?;
            if let Some(missing) = metadata.get("missingChunks") {
                if !missing.is_array() {
                    return Err(AppError::invalid_argument(
                        "capture metadata missingChunks must be an array",
                    ));
                }
            }
            Ok(json!({
                "kind": "capture",
                "dataUrl": data_url,
                "metadata": metadata,
                "capturedAtMs": response.get("capturedAtMs").cloned().unwrap_or(Value::Null),
                "verification": {
                    "authoritativeRevision": authoritative_revision,
                    "renderRevision": render_revision,
                    "stale": render_revision != authoritative_revision,
                    "partial": partial,
                },
            }))
        }
    }
}

fn validate_camera_pose(pose: &Value) -> Result<(), AppError> {
    for field in ["position", "target"] {
        let values = pose
            .get(field)
            .and_then(Value::as_array)
            .ok_or_else(|| AppError::invalid_argument(format!("camera pose has no {field}")))?;
        if values.len() != 3 || values.iter().any(|value| !value.is_number()) {
            return Err(AppError::invalid_argument(format!(
                "camera pose {field} must contain three numbers"
            )));
        }
    }
    for field in ["yaw", "pitch", "fov"] {
        if !pose.get(field).is_some_and(Value::is_number) {
            return Err(AppError::invalid_argument(format!(
                "camera pose has no numeric {field}"
            )));
        }
    }
    Ok(())
}

/// Marks the mounted viewport listener as ready. `ping` exposes this so a client can
/// distinguish "app started" from "a camera or capture request can be answered".
#[tauri::command]
pub fn renderer_ready(state: tauri::State<'_, AppState>) {
    state.renderer.mark_ready();
}

/// Replaces the MCP access policy. This is a Tauri/UI command, not a local-socket
/// method: an MCP client cannot grant itself edit access or widen its limits.
#[tauri::command]
pub fn set_mcp_access(
    state: tauri::State<'_, AppState>,
    input: McpAccessInput,
) -> Result<Value, AppError> {
    let mut summary = state.mcp_access.update(input)?;
    // An approval belongs to the policy that produced it. Narrowing (or even changing)
    // the policy must not let an already queued AI job commit under stale permissions.
    let cancelled = state
        .jobs
        .cancel_source(ActorSource::Mcp, "MCP access policy changed");
    summary["cancelledMcpJobs"] = json!(cancelled);
    Ok(summary)
}

/// Current policy plus any pending write approvals. The UI uses this to render its
/// access panel and to show a confirmation dialog after a reload.
#[tauri::command]
pub fn mcp_access_status(state: tauri::State<'_, AppState>) -> Value {
    json!({
        "policy": state.mcp_access.summary(),
        "pendingConfirmations": state.mcp_confirmations.pending_summaries(),
    })
}

/// Recent authenticated MCP calls for the open project. The UI never selects another
/// project and sees only metadata, not tool payloads.
#[tauri::command]
pub fn mcp_activity(
    state: tauri::State<'_, AppState>,
    operation_id: Option<String>,
    limit: Option<usize>,
) -> Result<Value, AppError> {
    let (project_id, _) = active_project_identity(&state)?;
    activity_response(&state, &project_id, operation_id.as_deref(), limit)
}

/// Answers a pending MCP write confirmation. Only the Tauri UI can call this; an MCP
/// client has no local-socket method that completes its own request.
#[tauri::command]
pub fn submit_mcp_confirmation(
    state: tauri::State<'_, AppState>,
    request_id: String,
    approved: bool,
) -> Result<(), AppError> {
    if request_id.trim().is_empty() || request_id.len() > MAX_REQUEST_ID_BYTES {
        return Err(AppError::invalid_argument(
            "confirmation request id must be 1..=128 characters",
        ));
    }
    if !state.mcp_confirmations.complete(&request_id, approved) {
        return Err(AppError::new(
            ErrorCode::NotFound,
            "confirmation request is unknown or expired",
        ));
    }
    Ok(())
}

/// Answers a pending viewport request. Called by the Tauri UI after it builds the
/// response. Unknown or expired ids are rejected instead of being queued forever.
#[tauri::command]
pub fn submit_renderer_response(
    state: tauri::State<'_, AppState>,
    request_id: String,
    response: Value,
) -> Result<(), AppError> {
    if request_id.trim().is_empty() || request_id.len() > MAX_REQUEST_ID_BYTES {
        return Err(AppError::invalid_argument(
            "renderer request id must be 1..=128 characters",
        ));
    }
    let encoded = serde_json::to_vec(&response).map_err(|error| {
        AppError::invalid_argument(format!("renderer response is not serializable: {error}"))
    })?;
    if encoded.len() > MAX_RENDERER_RESPONSE_BYTES {
        return Err(AppError::new(
            ErrorCode::LimitExceeded,
            "renderer response exceeds the size limit",
        ));
    }
    if !state.renderer.complete(&request_id, response) {
        return Err(AppError::new(
            ErrorCode::NotFound,
            "renderer request is unknown or expired",
        ));
    }
    Ok(())
}

struct ActivityMeta {
    request_id: String,
    client_id: Option<String>,
    method: String,
    project_id: Option<String>,
    requested_revision: Option<u64>,
    timestamp_ms: u64,
    started: Instant,
}

impl ActivityMeta {
    fn new(request_id: &str, client_id: Option<&str>, method: &str, params: &Value) -> Self {
        let request_id = clean_activity_text(request_id, MAX_REQUEST_ID_BYTES);
        let client_id = client_id
            .map(|value| clean_activity_text(value, MAX_REQUEST_ID_BYTES))
            .filter(|value| !value.is_empty());
        let method = clean_activity_text(method, 64);
        let project_id = params
            .get("projectId")
            .and_then(Value::as_str)
            .map(|value| clean_activity_text(value, 128))
            .filter(|value| !value.is_empty());
        let requested_revision = params.get("worldRevision").and_then(Value::as_u64);
        Self {
            request_id,
            client_id,
            method,
            project_id,
            requested_revision,
            timestamp_ms: now_ms(),
            started: Instant::now(),
        }
    }

    fn finish(self, outcome: &Result<Value, AppError>) -> McpActivityEntry {
        let (status, error_code, operation_id, changed_blocks, confirmation) = match outcome {
            Ok(value) => (
                McpActivityStatus::Ok,
                None,
                value
                    .get("operationId")
                    .and_then(Value::as_str)
                    .map(|id| clean_activity_text(id, 128)),
                value.get("changedBlocks").and_then(Value::as_u64),
                confirmation_label(value),
            ),
            Err(error) => (
                if matches!(
                    error.code,
                    ErrorCode::PermissionDenied | ErrorCode::Unauthorized
                ) {
                    McpActivityStatus::Denied
                } else {
                    McpActivityStatus::Error
                },
                Some(error.code),
                None,
                None,
                error.details.as_ref().and_then(confirmation_label),
            ),
        };
        McpActivityEntry {
            sequence: 0,
            request_id: self.request_id,
            client_id: self.client_id,
            method: self.method,
            status,
            error_code,
            project_id: self.project_id,
            requested_revision: self.requested_revision,
            operation_id,
            changed_blocks,
            confirmation,
            timestamp_ms: self.timestamp_ms,
            duration_ms: self.started.elapsed().as_millis() as u64,
        }
    }
}

fn confirmation_label(value: &Value) -> Option<String> {
    match value.get("confirmation")? {
        Value::String(label) => Some(clean_activity_text(label, 32)),
        Value::Object(details) => details
            .get("approved")
            .and_then(Value::as_bool)
            .map(|approved| if approved { "approved" } else { "denied" }.to_owned()),
        _ => None,
    }
}

fn parse_params<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, AppError> {
    serde_json::from_value(params)
        .map_err(|_| AppError::invalid_argument("bridge request parameters are invalid"))
}

fn to_value<T: Serialize>(value: T) -> Result<Value, AppError> {
    serde_json::to_value(value)
        .map_err(|error| AppError::internal(format!("serializing bridge result: {error}")))
}

fn active_project_identity(state: &AppState) -> Result<(String, Option<String>), AppError> {
    let projects = state
        .projects
        .lock()
        .map_err(|_| AppError::internal("project state lock poisoned"))?;
    Ok(match &projects.current {
        Some(current) => (current.project_id.clone(), Some(current.name.clone())),
        None => (LOCAL_PROJECT_ID.to_owned(), None),
    })
}

fn world_revision(state: &AppState) -> Result<u64, AppError> {
    let service = state
        .service
        .lock()
        .map_err(|_| AppError::internal("world state lock poisoned"))?;
    Ok(service.world().revision())
}

fn authorize_project(state: &AppState, requested: &str) -> Result<(), AppError> {
    let (active, _) = active_project_identity(state)?;
    if requested != active {
        return Err(AppError::new(
            ErrorCode::NotFound,
            "project_id does not match the project currently open in FuryBuilder",
        ));
    }
    Ok(())
}

/// A caller that acts on a snapshot must prove it is not looking at an older revision.
/// Omitting the revision remains valid for discovery reads such as `world_info`.
fn require_revision(state: &AppState, expected: Option<u64>) -> Result<(), AppError> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let actual = world_revision(state)?;
    if expected != actual {
        return Err(AppError::new(
            ErrorCode::RevisionMismatch,
            format!("requested world revision {expected}, but the open world is at {actual}"),
        )
        .with_details(json!({
            "expectedRevision": expected,
            "actualRevision": actual,
        })));
    }
    Ok(())
}

/// Compares the per-launch token without an early exit, so a wrong token cannot be
/// discovered byte-by-byte from response timing.
fn token_matches(expected: &str, provided: &str) -> bool {
    let expected = expected.as_bytes();
    let provided = provided.as_bytes();
    if expected.len() != provided.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (left, right) in expected.iter().zip(provided) {
        difference |= left ^ right;
    }
    difference == 0
}

fn endpoint_display_path(name: &str) -> String {
    if cfg!(windows) {
        format!(r"\\.\pipe\{name}")
    } else {
        name.to_owned()
    }
}

fn write_discovery_file(path: &PathBuf, runtime: &BridgeRuntimeFile) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(runtime)
        .map_err(|error| io::Error::other(format!("serializing bridge discovery: {error}")))?;
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4().simple()));
    fs::write(&temporary, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    }
    let _ = fs::remove_file(path);
    fs::rename(&temporary, path)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::state::AppState;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use world_application::Selection;
    use world_domain::{BlockBox, BlockPos};

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "furybuilder-bridge-{label}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn enable_writes(
        state: &AppState,
        mode: McpAccessMode,
        selection: Option<Selection>,
        max_blocks: u64,
        max_batch_changes: usize,
        require_confirmation: bool,
    ) {
        state
            .mcp_access
            .update(McpAccessInput {
                mode,
                selection,
                max_blocks: Some(max_blocks),
                max_batch_changes: Some(max_batch_changes),
                require_confirmation,
            })
            .expect("access policy");
    }

    fn single_set(pos: BlockPos) -> Value {
        json!({
            "type": "set_block",
            "dimension": "minecraft:overworld",
            "pos": pos,
            "state": { "name": "minecraft:stone" }
        })
    }

    struct ScriptedConfirmation {
        approve: bool,
        calls: AtomicUsize,
    }

    impl ScriptedConfirmation {
        fn new(approve: bool) -> Self {
            Self {
                approve,
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl RendererRequester for ScriptedConfirmation {
        fn request(&self, _state: &AppState, _request: RendererRequest) -> Result<Value, AppError> {
            Err(AppError::new(
                ErrorCode::BackendUnavailable,
                "no viewport in this test",
            ))
        }

        fn confirm(&self, _state: &AppState, _summary: Value) -> Result<bool, AppError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.approve)
        }
    }

    fn request(id: &str, token: &str, method: &str, params: Value) -> WireRequest {
        WireRequest {
            protocol: PROTOCOL_VERSION,
            id: id.to_owned(),
            token: token.to_owned(),
            method: method.to_owned(),
            client_id: None,
            params,
        }
    }

    #[test]
    fn rejects_a_wrong_token_without_echoing_it() {
        let state = AppState::new(temp_root("auth"));
        let secret = "a".repeat(64);
        let response = handle_request(
            &state,
            &secret,
            request("req-1", &"b".repeat(64), "ping", json!({})),
        );
        assert!(!response.ok);
        let error = response.error.expect("error");
        assert_eq!(error.code, ErrorCode::Unauthorized);
        let encoded = serde_json::to_string(&error).expect("serializable");
        assert!(!encoded.contains(&secret));
        assert!(!encoded.contains(&"b".repeat(64)));
    }

    #[test]
    fn rejects_unknown_protocol_versions() {
        let state = AppState::new(temp_root("protocol"));
        let mut wire = request("req-2", "token", "ping", json!({}));
        wire.protocol = 99;
        let response = handle_request(&state, "token", wire);
        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error").code,
            ErrorCode::UnsupportedVersion
        );
    }

    #[test]
    fn serves_world_info_and_bounded_queries_for_the_open_project() {
        let state = AppState::new(temp_root("query"));
        let response = handle_request(
            &state,
            "token",
            request(
                "req-3",
                "token",
                "world_info",
                json!({ "projectId": LOCAL_PROJECT_ID }),
            ),
        );
        assert!(response.ok, "{:?}", response.error);
        let info = response.result.expect("result");
        assert_eq!(info["targetMinecraftVersion"], "1.21.4");
        assert_eq!(info["revision"], 0);

        let response = handle_request(
            &state,
            "token",
            request(
                "req-4",
                "token",
                "query",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "query": {
                        "type": "get_block",
                        "dimension": "minecraft:overworld",
                        "pos": { "x": 0, "y": 64, "z": 0 }
                    }
                }),
            ),
        );
        assert!(response.ok, "{:?}", response.error);
        let result = response.result.expect("result");
        assert_eq!(result["type"], "block");
        assert_eq!(result["worldRevision"], 0);
        assert_eq!(result["data"]["state"]["name"], "minecraft:air");
    }

    #[test]
    fn rejects_stale_world_revisions_with_a_structured_conflict() {
        let state = AppState::new(temp_root("revision"));

        let current = handle_request(
            &state,
            "token",
            request(
                "req-current",
                "token",
                "world_info",
                json!({ "projectId": LOCAL_PROJECT_ID, "worldRevision": 0 }),
            ),
        );
        assert!(current.ok, "{:?}", current.error);

        let stale = handle_request(
            &state,
            "token",
            request(
                "req-stale",
                "token",
                "query",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "worldRevision": 99,
                    "query": { "type": "world_info" }
                }),
            ),
        );
        assert!(!stale.ok);
        let error = stale.error.expect("error");
        assert_eq!(error.code, ErrorCode::RevisionMismatch);
        let details = error.details.expect("details");
        assert_eq!(details["expectedRevision"], 99);
        assert_eq!(details["actualRevision"], 0);
    }

    #[test]
    fn query_observations_report_the_revision_they_read() {
        let state = AppState::new(temp_root("query-revision"));
        enable_writes(&state, McpAccessMode::FullProject, None, 1_000, 10, true);
        let approved = ScriptedConfirmation::new(true);
        let apply = handle_request_with_renderer(
            &state,
            "token",
            request(
                "req-query-seed",
                "token",
                "apply_operation",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "worldRevision": 0,
                    "idempotencyKey": "query-revision-seed",
                    "operation": {
                        "type": "batch",
                        "dimension": "minecraft:overworld",
                        "changes": [
                            { "pos": { "x": 0, "y": 64, "z": 0 }, "state": { "name": "minecraft:stone" } },
                            { "pos": { "x": 1, "y": 64, "z": 0 }, "state": { "name": "minecraft:glass" } }
                        ]
                    }
                }),
            ),
            &approved,
        );
        assert!(apply.ok, "{:?}", apply.error);

        let block = handle_request(
            &state,
            "token",
            request(
                "req-query-block",
                "token",
                "query",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "worldRevision": 1,
                    "query": {
                        "type": "get_block",
                        "dimension": "minecraft:overworld",
                        "pos": { "x": 1, "y": 64, "z": 0 }
                    }
                }),
            ),
        );
        assert!(block.ok, "{:?}", block.error);
        let block = block.result.expect("block");
        assert_eq!(block["worldRevision"], 1);
        assert_eq!(block["data"]["state"]["name"], "minecraft:glass");

        let summary = handle_request(
            &state,
            "token",
            request(
                "req-query-summary",
                "token",
                "query",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "query": {
                        "type": "region_summary",
                        "dimension": "minecraft:overworld",
                        "bounds": {
                            "min": { "x": -1, "y": 63, "z": -1 },
                            "max": { "x": 1, "y": 65, "z": 1 }
                        }
                    }
                }),
            ),
        );
        assert!(summary.ok, "{:?}", summary.error);
        let summary = summary.result.expect("summary");
        assert_eq!(summary["worldRevision"], 1);
        assert_eq!(summary["data"]["blocks"], 27);
        assert_eq!(summary["data"]["nonAirBlocks"], 2);
        assert!(summary["data"]["palette"]
            .as_array()
            .expect("palette")
            .iter()
            .any(|entry| entry["state"] == "minecraft:stone" && entry["count"] == 1));
        assert!(summary["data"]["palette"]
            .as_array()
            .expect("palette")
            .iter()
            .any(|entry| entry["state"] == "minecraft:glass" && entry["count"] == 1));

        let stale = handle_request(
            &state,
            "token",
            request(
                "req-query-stale",
                "token",
                "query",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "worldRevision": 0,
                    "query": {
                        "type": "get_block",
                        "dimension": "minecraft:overworld",
                        "pos": { "x": 1, "y": 64, "z": 0 }
                    }
                }),
            ),
        );
        assert!(!stale.ok);
        let error = stale.error.expect("stale error");
        assert_eq!(error.code, ErrorCode::RevisionMismatch);
        let details = error.details.expect("details");
        assert_eq!(details["expectedRevision"], 0);
        assert_eq!(details["actualRevision"], 1);
    }

    #[test]
    fn refuses_a_project_that_is_not_open() {
        let state = AppState::new(temp_root("project"));
        let response = handle_request(
            &state,
            "token",
            request(
                "req-5",
                "token",
                "world_info",
                json!({ "projectId": "another-project" }),
            ),
        );
        assert!(!response.ok);
        assert_eq!(response.error.expect("error").code, ErrorCode::NotFound);
    }

    #[test]
    fn rejects_malformed_query_parameters() {
        let state = AppState::new(temp_root("params"));
        let response = handle_request(
            &state,
            "token",
            request(
                "req-6",
                "token",
                "query",
                json!({ "projectId": LOCAL_PROJECT_ID, "query": { "type": "nope" } }),
            ),
        );
        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error").code,
            ErrorCode::InvalidArgument
        );
    }

    struct FakeRenderer {
        camera: Value,
        capture: Value,
    }

    impl RendererRequester for FakeRenderer {
        fn request(&self, _state: &AppState, request: RendererRequest) -> Result<Value, AppError> {
            Ok(match request.kind {
                RendererRequestKind::Camera => self.camera.clone(),
                RendererRequestKind::Capture => self.capture.clone(),
            })
        }
    }

    fn camera_response(revision: u64) -> Value {
        json!({
            "kind": "camera",
            "dimension": "minecraft:overworld",
            "worldRevision": revision,
            "pose": {
                "position": [1.0, 2.0, 3.0],
                "target": [0.0, 0.0, 0.0],
                "yaw": 0.5,
                "pitch": -0.2,
                "fov": 70.0
            },
            "capturedAtMs": 123
        })
    }

    fn capture_response(revision: u64, dimension: &str, data_url: &str) -> Value {
        json!({
            "kind": "capture",
            "dimension": dimension,
            "worldRevision": revision,
            "dataUrl": data_url,
            "metadata": {
                "dimension": dimension,
                "worldRevision": revision,
                "partial": false,
                "missingChunks": []
            },
            "capturedAtMs": 124
        })
    }

    #[test]
    fn serves_camera_and_capture_with_explicit_revision_verification() {
        let state = AppState::new(temp_root("renderer"));
        let renderer = FakeRenderer {
            camera: camera_response(0),
            capture: capture_response(
                7,
                "minecraft:overworld",
                &format!("{PNG_DATA_URL_PREFIX}iVBORw0KGgo="),
            ),
        };

        let camera = handle_request_with_renderer(
            &state,
            "token",
            request(
                "req-camera",
                "token",
                "get_camera",
                json!({ "projectId": LOCAL_PROJECT_ID }),
            ),
            &renderer,
        );
        assert!(camera.ok, "{:?}", camera.error);
        let camera = camera.result.expect("camera result");
        assert_eq!(camera["pose"]["position"][1], 2.0);
        assert_eq!(camera["verification"]["authoritativeRevision"], 0);
        assert_eq!(camera["verification"]["stale"], false);

        let capture = handle_request_with_renderer(
            &state,
            "token",
            request(
                "req-capture",
                "token",
                "capture_viewport",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "dimension": "minecraft:overworld"
                }),
            ),
            &renderer,
        );
        assert!(capture.ok, "{:?}", capture.error);
        let capture = capture.result.expect("capture result");
        assert!(capture["dataUrl"]
            .as_str()
            .expect("data url")
            .starts_with(PNG_DATA_URL_PREFIX));
        assert_eq!(capture["verification"]["renderRevision"], 7);
        assert_eq!(capture["verification"]["authoritativeRevision"], 0);
        assert_eq!(capture["verification"]["stale"], true);
        assert_eq!(capture["verification"]["partial"], false);
    }

    #[test]
    fn refuses_a_capture_for_a_dimension_the_viewport_is_not_showing() {
        let state = AppState::new(temp_root("renderer-dimension"));
        let renderer = FakeRenderer {
            camera: camera_response(0),
            capture: capture_response(
                0,
                "minecraft:the_nether",
                &format!("{PNG_DATA_URL_PREFIX}iVBORw0KGgo="),
            ),
        };
        let response = handle_request_with_renderer(
            &state,
            "token",
            request(
                "req-dimension",
                "token",
                "capture_viewport",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "dimension": "minecraft:overworld"
                }),
            ),
            &renderer,
        );
        assert!(!response.ok);
        let error = response.error.expect("error");
        assert_eq!(error.code, ErrorCode::NotFound);
        assert_eq!(
            error.details.expect("details")["actualDimension"],
            "minecraft:the_nether"
        );
    }

    #[test]
    fn rejects_an_invalid_viewport_capture_instead_of_returning_it() {
        let state = AppState::new(temp_root("renderer-invalid"));
        let renderer = FakeRenderer {
            camera: camera_response(0),
            capture: capture_response(0, "minecraft:overworld", "https://example.invalid/a.png"),
        };
        let response = handle_request_with_renderer(
            &state,
            "token",
            request(
                "req-invalid",
                "token",
                "capture_viewport",
                json!({ "projectId": LOCAL_PROJECT_ID }),
            ),
            &renderer,
        );
        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error").code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn fails_honestly_when_no_viewport_is_attached() {
        let state = AppState::new(temp_root("renderer-missing"));
        let response = handle_request(
            &state,
            "token",
            request(
                "req-missing",
                "token",
                "get_camera",
                json!({ "projectId": LOCAL_PROJECT_ID }),
            ),
        );
        assert!(!response.ok);
        assert_eq!(
            response.error.expect("error").code,
            ErrorCode::BackendUnavailable
        );
    }

    #[test]
    fn renderer_bridge_accepts_one_answer_per_request_and_rejects_expired_ids() {
        let bridge = RendererBridge::default();
        assert_eq!(bridge.readiness()["listening"], false);
        bridge.mark_ready();
        assert_eq!(bridge.readiness()["listening"], true);
        let receiver = bridge.open("known").expect("open");
        assert!(bridge.complete("known", json!({ "ok": true })));
        assert_eq!(receiver.recv().expect("response"), json!({ "ok": true }));
        assert!(!bridge.complete("known", json!({ "ok": false })));
        assert!(!bridge.complete("never-opened", json!({})));
    }

    #[test]
    fn preview_and_status_are_read_only_without_a_write_grant() {
        let state = AppState::new(temp_root("preview"));
        let preview = handle_request(
            &state,
            "token",
            request(
                "req-preview",
                "token",
                "preview_operation",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "operation": {
                        "type": "set_block",
                        "dimension": "minecraft:overworld",
                        "pos": { "x": 1, "y": 64, "z": 1 },
                        "state": { "name": "minecraft:stone" }
                    }
                }),
            ),
        );
        assert!(preview.ok, "{:?}", preview.error);
        let preview = preview.result.expect("preview");
        assert_eq!(preview["affectedBlocks"], 1);
        assert_eq!(preview["changedBlocks"], 1);
        assert_eq!(preview["worldRevision"], 0);

        let revision = {
            let service = state.service.lock().expect("service");
            service.world().revision()
        };
        assert_eq!(revision, 0, "preview must not mutate the world");

        let status = handle_request(
            &state,
            "token",
            request(
                "req-status",
                "token",
                "operation_status",
                json!({ "projectId": LOCAL_PROJECT_ID }),
            ),
        );
        assert!(status.ok, "{:?}", status.error);
        let status = status.result.expect("status");
        assert_eq!(status["currentRevision"], 0);
        assert_eq!(status["canUndo"], false);
        assert_eq!(status["job"]["available"], true);
        assert_eq!(status["job"]["summary"]["queued"], 0);
        assert!(status["job"]["recent"]
            .as_array()
            .expect("recent jobs")
            .is_empty());
    }

    fn wait_for_job(state: &AppState, job_id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let response = handle_request(
                state,
                "token",
                request(
                    "req-job-poll",
                    "token",
                    "job_status",
                    json!({ "projectId": LOCAL_PROJECT_ID, "jobId": job_id }),
                ),
            );
            assert!(response.ok, "{:?}", response.error);
            let snapshot = response.result.expect("job status");
            let label = snapshot["state"].as_str().unwrap_or_default();
            if matches!(label, "succeeded" | "failed" | "cancelled") {
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "job did not finish in time: {snapshot}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn jobs_run_apply_with_progress_and_link_retries_to_the_operation() {
        let state = AppState::new(temp_root("jobs"));
        enable_writes(&state, McpAccessMode::FullProject, None, 1_000, 10, false);
        let params = json!({
            "projectId": LOCAL_PROJECT_ID,
            "kind": "apply_operation",
            "parameters": { "operation": single_set(BlockPos::new(8, 64, 8)) },
            "idempotencyKey": "job-apply-1",
            "worldRevision": 0
        });

        let started = handle_request(
            &state,
            "token",
            request("req-job-start", "token", "start_job", params.clone()),
        );
        assert!(started.ok, "{:?}", started.error);
        let started = started.result.expect("started job");
        let job_id = started["jobId"].as_str().expect("job id").to_owned();
        assert_eq!(started["kind"], "apply_operation");
        assert_eq!(started["projectId"], LOCAL_PROJECT_ID);

        let finished = wait_for_job(&state, &job_id);
        assert_eq!(finished["state"], "succeeded", "{finished}");
        assert_eq!(finished["progress"], 100);
        assert_eq!(finished["worldRevision"], 1);
        assert_eq!(finished["result"]["changedBlocks"], 1);
        let operation_id = finished["result"]["operationId"]
            .as_str()
            .expect("operation id")
            .to_owned();

        let retry = handle_request(
            &state,
            "token",
            request("req-job-retry", "token", "start_job", params),
        );
        assert!(retry.ok, "{:?}", retry.error);
        let retry = retry.result.expect("retried job");
        assert_eq!(retry["jobId"], job_id);
        assert_eq!(retry["replayed"], true);
        let revision = {
            let service = state.service.lock().expect("service");
            service.world().revision()
        };
        assert_eq!(revision, 1, "an idempotent retry must not write twice");

        let list = handle_request(
            &state,
            "token",
            request(
                "req-job-list",
                "token",
                "list_jobs",
                json!({ "projectId": LOCAL_PROJECT_ID, "limit": 10 }),
            ),
        );
        assert!(list.ok, "{:?}", list.error);
        let list = list.result.expect("job list");
        assert_eq!(list["jobs"].as_array().expect("jobs").len(), 1);
        assert_eq!(list["jobs"][0]["jobId"], job_id);
        assert_eq!(list["summary"]["succeeded"], 1);

        let status = handle_request(
            &state,
            "token",
            request(
                "req-job-linked-status",
                "token",
                "operation_status",
                json!({ "projectId": LOCAL_PROJECT_ID, "operationId": operation_id }),
            ),
        );
        assert!(status.ok, "{:?}", status.error);
        let status = status.result.expect("operation status");
        assert_eq!(status["job"]["available"], true);
        assert_eq!(status["job"]["operationJob"]["jobId"], job_id);
        assert_eq!(status["job"]["summary"]["succeeded"], 1);

        let invalid = handle_request(
            &state,
            "token",
            request(
                "req-job-list-limit",
                "token",
                "list_jobs",
                json!({ "projectId": LOCAL_PROJECT_ID, "limit": 0 }),
            ),
        );
        assert!(!invalid.ok);
        assert_eq!(
            invalid.error.expect("limit error").code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn jobs_require_write_access_and_keep_the_same_confirmation_gate() {
        let state = AppState::new(temp_root("jobs-denied"));
        let params = json!({
            "projectId": LOCAL_PROJECT_ID,
            "kind": "apply_operation",
            "parameters": { "operation": single_set(BlockPos::new(9, 64, 9)) },
            "idempotencyKey": "job-denied-1",
            "worldRevision": 0
        });
        let denied = handle_request(
            &state,
            "token",
            request("req-job-read-only", "token", "start_job", params.clone()),
        );
        assert!(!denied.ok);
        let error = denied.error.expect("read-only error");
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert_eq!(
            error.details.expect("details")["mcpAccessMode"],
            "read_only"
        );

        enable_writes(&state, McpAccessMode::FullProject, None, 1_000, 10, true);
        let refused = ScriptedConfirmation::new(false);
        let response = handle_request_with_renderer(
            &state,
            "token",
            request("req-job-deny", "token", "start_job", params.clone()),
            &refused,
        );
        assert!(!response.ok);
        assert_eq!(
            response.error.expect("denied job").code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(refused.calls.load(Ordering::SeqCst), 1);
        assert!(state.jobs.list(LOCAL_PROJECT_ID, 10).is_empty());

        let approved = ScriptedConfirmation::new(true);
        let response = handle_request_with_renderer(
            &state,
            "token",
            request("req-job-approve", "token", "start_job", params.clone()),
            &approved,
        );
        assert!(response.ok, "{:?}", response.error);
        let started = response.result.expect("approved job");
        assert_eq!(started["confirmation"]["approved"], true);
        assert_eq!(started["confirmation"]["deferred"], true);
        let job_id = started["jobId"].as_str().expect("job id").to_owned();
        let finished = wait_for_job(&state, &job_id);
        assert_eq!(finished["state"], "succeeded", "{finished}");
        assert_eq!(approved.calls.load(Ordering::SeqCst), 1);

        let retry = ScriptedConfirmation::new(true);
        let response = handle_request_with_renderer(
            &state,
            "token",
            request("req-job-retry-approved", "token", "start_job", params),
            &retry,
        );
        assert!(response.ok, "{:?}", response.error);
        assert_eq!(
            response.result.expect("retry")["jobId"],
            job_id,
            "a retry returns the existing job"
        );
        assert_eq!(
            retry.calls.load(Ordering::SeqCst),
            0,
            "an idempotent retry must not ask for approval again"
        );
    }

    #[test]
    fn job_status_and_cancel_are_scoped_to_the_open_project() {
        let state = AppState::new(temp_root("job-scope"));
        let unknown = handle_request(
            &state,
            "token",
            request(
                "req-job-unknown",
                "token",
                "job_status",
                json!({ "projectId": LOCAL_PROJECT_ID, "jobId": "job-missing" }),
            ),
        );
        assert!(!unknown.ok);
        assert_eq!(
            unknown.error.expect("unknown job").code,
            ErrorCode::NotFound
        );

        let wrong_project = handle_request(
            &state,
            "token",
            request(
                "req-job-project",
                "token",
                "list_jobs",
                json!({ "projectId": "another-project", "limit": 10 }),
            ),
        );
        assert!(!wrong_project.ok);
        assert_eq!(
            wrong_project.error.expect("wrong project").code,
            ErrorCode::NotFound
        );

        let cancel = handle_request(
            &state,
            "token",
            request(
                "req-job-cancel-missing",
                "token",
                "cancel_job",
                json!({ "projectId": LOCAL_PROJECT_ID, "jobId": "job-missing" }),
            ),
        );
        assert!(!cancel.ok);
        assert_eq!(
            cancel.error.expect("cancel missing").code,
            ErrorCode::NotFound
        );
    }

    #[test]
    fn read_only_default_rejects_writes_and_reports_policy() {
        let state = AppState::new(temp_root("write-denied"));
        let operation = single_set(BlockPos::new(1, 64, 1));

        let preview = handle_request(
            &state,
            "token",
            request(
                "req-preview-denied",
                "token",
                "preview_operation",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "worldRevision": 0,
                    "operation": operation.clone()
                }),
            ),
        );
        assert!(preview.ok, "{:?}", preview.error);
        let preview = preview.result.expect("preview");
        assert_eq!(preview["access"]["allowed"], false);
        assert_eq!(preview["access"]["code"], "permission_denied");
        assert_eq!(preview["access"]["details"]["mcpAccessMode"], "read_only");
        assert_eq!(preview["affectedBlocks"], 1);

        let apply = handle_request(
            &state,
            "token",
            request(
                "req-apply-denied",
                "token",
                "apply_operation",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "worldRevision": 0,
                    "operation": operation,
                    "idempotencyKey": "denied-1"
                }),
            ),
        );
        assert!(!apply.ok);
        let error = apply.error.expect("error");
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert_eq!(
            error.details.expect("details")["mcpAccessMode"],
            "read_only"
        );

        let undo = handle_request(
            &state,
            "token",
            request(
                "req-undo-denied",
                "token",
                "undo_operation",
                json!({ "projectId": LOCAL_PROJECT_ID, "worldRevision": 0 }),
            ),
        );
        assert!(!undo.ok);
        let error = undo.error.expect("error");
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert_eq!(
            error.details.expect("details")["mcpAccessMode"],
            "read_only"
        );

        let ping = handle_request(
            &state,
            "token",
            request("req-ping", "token", "ping", json!({})),
        );
        let ping = ping.result.expect("ping");
        assert_eq!(ping["mcp"]["mode"], "read_only");
        assert_eq!(ping["mcp"]["writesEnabled"], false);
        assert_eq!(ping["pendingConfirmations"], 0);

        let revision = {
            let service = state.service.lock().expect("service");
            service.world().revision()
        };
        assert_eq!(revision, 0);
    }

    #[test]
    fn edit_in_selection_enforces_box_and_limits() {
        let state = AppState::new(temp_root("write-selection"));
        enable_writes(
            &state,
            McpAccessMode::EditInSelection,
            Some(Selection {
                dimension: "minecraft:overworld".into(),
                bounds: BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(15, 75, 15))
                    .expect("selection"),
            }),
            10_000,
            2,
            false,
        );

        let inside = handle_request(
            &state,
            "token",
            request(
                "req-inside",
                "token",
                "apply_operation",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "worldRevision": 0,
                    "idempotencyKey": "inside-1",
                    "operation": single_set(BlockPos::new(5, 64, 5))
                }),
            ),
        );
        assert!(inside.ok, "{:?}", inside.error);
        assert_eq!(inside.result.expect("inside")["worldRevision"], 1);

        let outside = handle_request(
            &state,
            "token",
            request(
                "req-outside",
                "token",
                "apply_operation",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "worldRevision": 1,
                    "idempotencyKey": "outside-1",
                    "operation": single_set(BlockPos::new(40, 64, 5))
                }),
            ),
        );
        assert!(!outside.ok);
        let error = outside.error.expect("outside error");
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert_eq!(
            error.details.expect("details")["mcpAccessMode"],
            "edit_in_selection"
        );

        let too_many = handle_request(
            &state,
            "token",
            request(
                "req-batch-limit",
                "token",
                "apply_operation",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "worldRevision": 1,
                    "idempotencyKey": "batch-too-large",
                    "operation": {
                        "type": "batch",
                        "dimension": "minecraft:overworld",
                        "changes": [
                            { "pos": { "x": 1, "y": 64, "z": 1 }, "state": { "name": "minecraft:stone" } },
                            { "pos": { "x": 2, "y": 64, "z": 1 }, "state": { "name": "minecraft:stone" } },
                            { "pos": { "x": 3, "y": 64, "z": 1 }, "state": { "name": "minecraft:stone" } }
                        ]
                    }
                }),
            ),
        );
        assert!(!too_many.ok);
        assert_eq!(
            too_many.error.expect("batch error").code,
            ErrorCode::LimitExceeded
        );

        let revision = {
            let service = state.service.lock().expect("service");
            service.world().revision()
        };
        assert_eq!(revision, 1, "rejected writes must not bump the revision");
    }

    #[test]
    fn write_requires_explicit_world_revision() {
        let state = AppState::new(temp_root("write-revision"));
        enable_writes(&state, McpAccessMode::FullProject, None, 1_000, 10, false);
        let apply = handle_request(
            &state,
            "token",
            request(
                "req-no-revision",
                "token",
                "apply_operation",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "idempotencyKey": "no-revision",
                    "operation": single_set(BlockPos::new(1, 64, 1))
                }),
            ),
        );
        assert!(!apply.ok);
        assert_eq!(
            apply.error.expect("revision error").code,
            ErrorCode::InvalidArgument
        );
        let revision = {
            let service = state.service.lock().expect("service");
            service.world().revision()
        };
        assert_eq!(revision, 0);
    }

    #[test]
    fn confirmation_can_deny_or_approve_a_write() {
        let state = AppState::new(temp_root("write-confirmation"));
        enable_writes(&state, McpAccessMode::FullProject, None, 1_000, 10, true);
        let params = json!({
            "projectId": LOCAL_PROJECT_ID,
            "worldRevision": 0,
            "idempotencyKey": "confirmed-1",
            "operation": single_set(BlockPos::new(1, 64, 1))
        });

        let denied = ScriptedConfirmation::new(false);
        let response = handle_request_with_renderer(
            &state,
            "token",
            request(
                "req-confirm-denied",
                "token",
                "apply_operation",
                params.clone(),
            ),
            &denied,
        );
        assert!(!response.ok);
        let error = response.error.expect("denied error");
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert_eq!(error.details.expect("details")["confirmation"], "denied");
        assert_eq!(denied.calls.load(Ordering::SeqCst), 1);
        let revision = {
            let service = state.service.lock().expect("service");
            service.world().revision()
        };
        assert_eq!(revision, 0);

        let approved = ScriptedConfirmation::new(true);
        let response = handle_request_with_renderer(
            &state,
            "token",
            request("req-confirm-ok", "token", "apply_operation", params),
            &approved,
        );
        assert!(response.ok, "{:?}", response.error);
        assert_eq!(response.result.expect("apply result")["worldRevision"], 1);
        assert_eq!(approved.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn undo_confirmation_covers_the_exact_operation() {
        let state = AppState::new(temp_root("undo-confirmation"));
        enable_writes(&state, McpAccessMode::FullProject, None, 1_000, 10, false);
        let apply = handle_request(
            &state,
            "token",
            request(
                "req-apply-before-undo",
                "token",
                "apply_operation",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "worldRevision": 0,
                    "idempotencyKey": "undo-source",
                    "operation": {
                        "type": "batch",
                        "dimension": "minecraft:overworld",
                        "changes": [
                            { "pos": { "x": 1, "y": 64, "z": 1 }, "state": { "name": "minecraft:stone" } },
                            { "pos": { "x": 2, "y": 64, "z": 1 }, "state": { "name": "minecraft:glass" } }
                        ]
                    }
                }),
            ),
        );
        assert!(apply.ok, "{:?}", apply.error);
        let operation_id = apply.result.expect("apply")["operationId"]
            .as_str()
            .expect("operation id")
            .to_owned();

        enable_writes(&state, McpAccessMode::FullProject, None, 1_000, 10, true);
        let undo_params = json!({
            "projectId": LOCAL_PROJECT_ID,
            "worldRevision": 1,
            "operationId": operation_id
        });
        let denied = ScriptedConfirmation::new(false);
        let response = handle_request_with_renderer(
            &state,
            "token",
            request(
                "req-undo-denied",
                "token",
                "undo_operation",
                undo_params.clone(),
            ),
            &denied,
        );
        assert!(!response.ok);
        assert_eq!(
            response.error.expect("denied").code,
            ErrorCode::PermissionDenied
        );
        assert_eq!(denied.calls.load(Ordering::SeqCst), 1);

        let approved = ScriptedConfirmation::new(true);
        let response = handle_request_with_renderer(
            &state,
            "token",
            request("req-undo-approved", "token", "undo_operation", undo_params),
            &approved,
        );
        assert!(response.ok, "{:?}", response.error);
        let result = response.result.expect("undo result");
        assert_eq!(result["changedBlocks"], 2);
        assert_eq!(result["worldRevision"], 2);
        assert_eq!(approved.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn activity_log_links_tool_calls_to_operations_and_errors() {
        let state = AppState::new(temp_root("activity"));
        let operation = single_set(BlockPos::new(3, 64, 3));

        let mut denied_request = request(
            "req-denied-activity",
            "token",
            "apply_operation",
            json!({
                "projectId": LOCAL_PROJECT_ID,
                "worldRevision": 0,
                "operation": operation.clone(),
                "idempotencyKey": "activity-denied"
            }),
        );
        denied_request.client_id = Some("bridge-test".to_owned());
        let denied = handle_request(&state, "token", denied_request);
        assert!(!denied.ok);

        enable_writes(&state, McpAccessMode::FullProject, None, 1_000, 10, false);
        let mut applied_request = request(
            "req-applied-activity",
            "token",
            "apply_operation",
            json!({
                "projectId": LOCAL_PROJECT_ID,
                "worldRevision": 0,
                "operation": operation,
                "idempotencyKey": "activity-applied"
            }),
        );
        applied_request.client_id = Some("bridge-test".to_owned());
        let applied = handle_request(&state, "token", applied_request);
        assert!(applied.ok, "{:?}", applied.error);
        let operation_id = applied.result.expect("apply")["operationId"]
            .as_str()
            .expect("operation id")
            .to_owned();

        let activity = handle_request(
            &state,
            "token",
            request(
                "req-activity",
                "token",
                "mcp_activity",
                json!({ "projectId": LOCAL_PROJECT_ID, "limit": 10 }),
            ),
        );
        assert!(activity.ok, "{:?}", activity.error);
        let activity = activity.result.expect("activity");
        let entries = activity["entries"].as_array().expect("entries");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["requestId"], "req-applied-activity");
        assert_eq!(entries[0]["status"], "ok");
        assert_eq!(entries[0]["clientId"], "bridge-test");
        assert_eq!(entries[0]["operationId"], operation_id);
        assert_eq!(entries[0]["changedBlocks"], 1);
        assert_eq!(entries[1]["requestId"], "req-denied-activity");
        assert_eq!(entries[1]["status"], "denied");
        assert_eq!(entries[1]["errorCode"], "permission_denied");
        assert_eq!(activity["stored"], 2);

        let status = handle_request(
            &state,
            "token",
            request(
                "req-activity-status",
                "token",
                "operation_status",
                json!({ "projectId": LOCAL_PROJECT_ID, "operationId": operation_id }),
            ),
        );
        assert!(status.ok, "{:?}", status.error);
        let status = status.result.expect("status");
        let linked = status["mcpActivity"].as_array().expect("linked activity");
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0]["requestId"], "req-applied-activity");

        let filtered = handle_request(
            &state,
            "token",
            request(
                "req-activity-filter",
                "token",
                "mcp_activity",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "operationId": operation_id
                }),
            ),
        );
        let filtered = filtered.result.expect("filtered activity");
        let filtered = filtered["entries"].as_array().expect("entries");
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0]["method"], "operation_status");
        assert_eq!(filtered[1]["requestId"], "req-applied-activity");

        let invalid = handle_request(
            &state,
            "token",
            request(
                "req-activity-limit",
                "token",
                "mcp_activity",
                json!({ "projectId": LOCAL_PROJECT_ID, "limit": 0 }),
            ),
        );
        assert!(!invalid.ok);
        assert_eq!(
            invalid.error.expect("limit error").code,
            ErrorCode::InvalidArgument
        );
    }

    #[test]
    fn granted_batch_apply_is_audited_and_can_be_undone() {
        let state = AppState::new(temp_root("write-granted"));
        enable_writes(&state, McpAccessMode::FullProject, None, 10_000, 100, false);

        let apply = handle_request(
            &state,
            "token",
            request(
                "req-apply",
                "token",
                "apply_operation",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "worldRevision": 0,
                    "idempotencyKey": "batch-1",
                    "operation": {
                        "type": "batch",
                        "dimension": "minecraft:overworld",
                        "changes": [
                            {
                                "pos": { "x": 1, "y": 64, "z": 1 },
                                "state": { "name": "minecraft:stone" }
                            },
                            {
                                "pos": { "x": 2, "y": 64, "z": 1 },
                                "state": { "name": "minecraft:oak_planks" }
                            }
                        ]
                    }
                }),
            ),
        );
        assert!(apply.ok, "{:?}", apply.error);
        let apply = apply.result.expect("apply result");
        assert_eq!(apply["changedBlocks"], 2);
        assert_eq!(apply["worldRevision"], 1);
        let operation_id = apply["operationId"]
            .as_str()
            .expect("operation id")
            .to_owned();

        let status = handle_request(
            &state,
            "token",
            request(
                "req-applied-status",
                "token",
                "operation_status",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "operationId": operation_id
                }),
            ),
        );
        assert!(status.ok, "{:?}", status.error);
        let status = status.result.expect("status");
        assert_eq!(status["action"], "apply");
        assert_eq!(status["source"], "mcp");
        assert_eq!(status["actorId"], "mcp-bridge");
        assert_eq!(status["changedBlocks"], 2);
        assert_eq!(status["job"]["available"], true);
        assert!(
            status["job"]["operationJob"].is_null(),
            "a synchronous write has no job record"
        );

        let undo = handle_request(
            &state,
            "token",
            request(
                "req-undo",
                "token",
                "undo_operation",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "worldRevision": 1,
                    "operationId": operation_id
                }),
            ),
        );
        assert!(undo.ok, "{:?}", undo.error);
        let undo = undo.result.expect("undo result");
        assert_eq!(undo["changedBlocks"], 2);
        assert_eq!(undo["worldRevision"], 2);

        let block = handle_request(
            &state,
            "token",
            request(
                "req-block-after-undo",
                "token",
                "query",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "query": {
                        "type": "get_block",
                        "dimension": "minecraft:overworld",
                        "pos": { "x": 1, "y": 64, "z": 1 }
                    }
                }),
            ),
        );
        assert!(block.ok, "{:?}", block.error);
        let block = block.result.expect("block");
        assert_eq!(block["data"]["state"]["name"], "minecraft:air");
    }

    #[test]
    fn local_socket_round_trip_reaches_the_authoritative_service() {
        let state = AppState::new(temp_root("socket"));
        let suffix = Uuid::new_v4().simple().to_string();
        let endpoint = format!(
            "furybuilder-bridge-test-{}-{}",
            std::process::id(),
            &suffix[..12]
        );
        let name = endpoint
            .as_str()
            .to_ns_name::<GenericNamespaced>()
            .expect("namespace name");
        let listener = ListenerOptions::new()
            .name(name)
            .create_sync()
            .expect("listener");

        std::thread::scope(|scope| {
            let server = scope.spawn(|| {
                let connection = listener
                    .incoming()
                    .next()
                    .expect("connection")
                    .expect("stream");
                serve_connection(connection, "token", &state, &UnavailableRenderer).expect("serve");
            });

            let name = endpoint
                .as_str()
                .to_ns_name::<GenericNamespaced>()
                .expect("namespace name");
            let stream = Stream::connect(name).expect("connect");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut writer = stream;
            let payload = serde_json::to_vec(&json!({
                "protocol": PROTOCOL_VERSION,
                "id": "round-trip",
                "token": "token",
                "method": "world_info",
                "params": { "projectId": LOCAL_PROJECT_ID }
            }))
            .expect("request json");
            writer.write_all(&payload).expect("write");
            writer.write_all(b"\n").expect("newline");
            writer.flush().expect("flush");

            let mut line = String::new();
            reader.read_line(&mut line).expect("read response");
            let response: WireResponse = serde_json::from_str(line.trim()).expect("response json");
            assert!(response.ok, "{:?}", response.error);
            assert_eq!(response.id, "round-trip");

            server.join().expect("server thread");
        });
    }

    #[test]
    fn discovery_file_round_trips_and_is_removed_by_cleanup() {
        let root = temp_root("discovery");
        fs::create_dir_all(&root).expect("dir");
        let path = root.join("runtime.json");
        let runtime = BridgeRuntimeFile {
            schema_version: DISCOVERY_SCHEMA_VERSION,
            transport: "local_socket".to_owned(),
            endpoint: endpoint_display_path("furybuilder-test"),
            token: "secret".to_owned(),
            pid: std::process::id(),
            protocol_version: PROTOCOL_VERSION,
            app_version: "0.1.0".to_owned(),
            started_at_ms: now_ms(),
        };
        write_discovery_file(&path, &runtime).expect("write");
        let stored = LocalBridgeRuntime {
            discovery_path: path.clone(),
        };
        let parsed: BridgeRuntimeFile =
            serde_json::from_slice(&fs::read(&path).expect("read")).expect("parse");
        assert_eq!(parsed.schema_version, DISCOVERY_SCHEMA_VERSION);
        assert_eq!(parsed.protocol_version, PROTOCOL_VERSION);
        stored.cleanup();
        std::thread::sleep(Duration::from_millis(1));
        assert!(!path.exists());
        let _ = fs::remove_dir_all(&root);
    }
}
