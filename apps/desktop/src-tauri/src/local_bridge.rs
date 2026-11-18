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
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericNamespaced, Listener, ListenerOptions, Stream};
use interprocess::TryClone;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;
use world_application::{
    app_info, Actor, AppError, ErrorCode, Operation, OperationRequest, Query, WorldService,
};

use crate::state::{session_status, AppState, LOCAL_PROJECT_ID};

pub const PROTOCOL_VERSION: u32 = 1;
pub const DISCOVERY_SCHEMA_VERSION: u32 = 1;
pub const RENDERER_REQUEST_EVENT: &str = "furybuilder-renderer-request";

const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_REQUEST_ID_BYTES: usize = 128;
const DISCOVERY_DIR: &str = "mcp";
const DISCOVERY_FILE: &str = "runtime.json";
const MAX_CAPTURE_DATA_URL_BYTES: usize = 12 * 1024 * 1024;
const MAX_RENDERER_RESPONSE_BYTES: usize = MAX_CAPTURE_DATA_URL_BYTES + 256 * 1024;
const MAX_PENDING_RENDERER_REQUESTS: usize = 8;
const RENDERER_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
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

/// Explicit user grant for MCP writes. The bridge cannot grant it: only the Tauri UI
/// calls `set_mcp_writes_enabled`. Until then apply/undo are refused by the endpoint.
/// Modes, selections and per-request confirmation arrive in the access-policy task.
#[derive(Debug, Default)]
pub struct McpWriteAccess {
    granted_at_ms: std::sync::Mutex<Option<u64>>,
}

impl McpWriteAccess {
    fn granted(&self) -> bool {
        self.granted_at_ms
            .lock()
            .ok()
            .is_some_and(|granted| granted.is_some())
    }

    fn set(&self, enabled: bool) {
        if let Ok(mut granted) = self.granted_at_ms.lock() {
            *granted = enabled.then(now_ms);
        }
    }

    pub fn summary(&self) -> Value {
        let granted_at_ms = self.granted_at_ms.lock().ok().and_then(|granted| *granted);
        json!({
            "writesEnabled": granted_at_ms.is_some(),
            "grantedAtMs": granted_at_ms,
        })
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

/// Starts the local endpoint. Failure is reported to the caller instead of panicking so a
/// missing pipe cannot stop the editor from opening; the UI remains fully usable.
pub fn start(app: AppHandle) -> io::Result<LocalBridgeRuntime> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| io::Error::other(format!("app data directory: {error}")))?;
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

    match dispatch(state, &request.method, request.params, requester) {
        Ok(result) => WireResponse::ok(id, result),
        Err(error) => WireResponse::error(id, error),
    }
}

fn dispatch(
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
                "mcp": state.mcp_writes.summary(),
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
            to_value(service.query(params.query)?)
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
            Ok(value)
        }
        "apply_operation" => {
            let params: OperationParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            require_mcp_write_access(state)?;
            require_revision(state, params.world_revision)?;
            let mut service = state
                .service
                .lock()
                .map_err(|_| AppError::internal("world state lock poisoned"))?;
            to_value(service.execute(OperationRequest {
                project_id: params.project_id,
                operation: params.operation,
                expected_revision: params.world_revision,
                idempotency_key: params.idempotency_key,
                actor: Actor::mcp("mcp-bridge"),
                linked_object_id: None,
            })?)
        }
        "undo_operation" => {
            let params: UndoParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            require_mcp_write_access(state)?;
            require_revision(state, params.world_revision)?;
            let mut service = state
                .service
                .lock()
                .map_err(|_| AppError::internal("world state lock poisoned"))?;
            to_value(service.undo(
                &params.project_id,
                Actor::mcp("mcp-bridge"),
                params.operation_id.as_deref(),
            )?)
        }
        "operation_status" => {
            let params: StatusParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            let service = state
                .service
                .lock()
                .map_err(|_| AppError::internal("world state lock poisoned"))?;
            operation_status(&service, params.operation_id.as_deref())
        }
        other => Err(AppError::invalid_argument(format!(
            "unknown bridge method `{other}`"
        ))),
    }
}

fn require_mcp_write_access(state: &AppState) -> Result<(), AppError> {
    if state.mcp_writes.granted() {
        return Ok(());
    }
    Err(AppError::new(
        ErrorCode::PermissionDenied,
        "MCP write access is not enabled; enable it explicitly in FuryBuilder",
    )
    .with_details(json!({ "mcpWritesEnabled": false })))
}

/// Operations are synchronous, so their authoritative status lives in the audit log.
/// Jobs/progress are explicitly unavailable rather than simulated.
fn operation_status(service: &WorldService, operation_id: Option<&str>) -> Result<Value, AppError> {
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
            "job": {
                "available": false,
                "progress": null,
                "reason": "job_api_unavailable"
            },
        }));
    }

    let recent: Vec<_> = service.history().into_iter().rev().take(10).collect();
    Ok(json!({
        "status": "idle",
        "currentRevision": revision,
        "canUndo": service.can_undo(),
        "canRedo": service.can_redo(),
        "recentOperations": recent,
        "job": {
            "available": false,
            "progress": null,
            "reason": "job_api_unavailable"
        },
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

/// Grants or revokes MCP write access. This is a Tauri/UI command, not a local-socket
/// method: an MCP client cannot call it to authorize itself.
#[tauri::command]
pub fn set_mcp_writes_enabled(state: tauri::State<'_, AppState>, enabled: bool) -> Value {
    state.mcp_writes.set(enabled);
    state.mcp_writes.summary()
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
    use std::time::Duration;

    use super::*;
    use crate::state::AppState;

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "furybuilder-bridge-{label}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = fs::remove_dir_all(&root);
        root
    }

    fn request(id: &str, token: &str, method: &str, params: Value) -> WireRequest {
        WireRequest {
            protocol: PROTOCOL_VERSION,
            id: id.to_owned(),
            token: token.to_owned(),
            method: method.to_owned(),
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
        assert_eq!(status["job"]["available"], false);
        assert_eq!(status["job"]["reason"], "job_api_unavailable");
    }

    #[test]
    fn apply_and_undo_are_refused_until_mcp_write_access_is_granted() {
        let state = AppState::new(temp_root("write-denied"));
        let operation = json!({
            "type": "set_block",
            "dimension": "minecraft:overworld",
            "pos": { "x": 1, "y": 64, "z": 1 },
            "state": { "name": "minecraft:stone" }
        });

        let apply = handle_request(
            &state,
            "token",
            request(
                "req-apply-denied",
                "token",
                "apply_operation",
                json!({
                    "projectId": LOCAL_PROJECT_ID,
                    "operation": operation.clone(),
                    "idempotencyKey": "denied-1"
                }),
            ),
        );
        assert!(!apply.ok);
        let error = apply.error.expect("error");
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert_eq!(error.details.expect("details")["mcpWritesEnabled"], false);

        let undo = handle_request(
            &state,
            "token",
            request(
                "req-undo-denied",
                "token",
                "undo_operation",
                json!({ "projectId": LOCAL_PROJECT_ID }),
            ),
        );
        assert!(!undo.ok);
        assert_eq!(undo.error.expect("error").code, ErrorCode::PermissionDenied);

        let revision = {
            let service = state.service.lock().expect("service");
            service.world().revision()
        };
        assert_eq!(revision, 0);
    }

    #[test]
    fn granted_batch_apply_is_audited_and_can_be_undone() {
        let state = AppState::new(temp_root("write-granted"));
        state.mcp_writes.set(true);

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
        assert_eq!(status["job"]["available"], false);

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
    fn mcp_write_grant_summary_reflects_the_explicit_toggle() {
        let access = McpWriteAccess::default();
        assert!(!access.granted());
        assert_eq!(access.summary()["writesEnabled"], false);
        access.set(true);
        assert!(access.granted());
        assert_eq!(access.summary()["writesEnabled"], true);
        access.set(false);
        assert!(!access.granted());
        assert_eq!(access.summary()["writesEnabled"], false);
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
