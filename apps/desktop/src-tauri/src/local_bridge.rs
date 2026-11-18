//! Local authenticated IPC endpoint for the MCP bridge.
//!
//! The endpoint is a local socket (a Windows named pipe on the first target platform)
//! created by the running editor. A user-private discovery file publishes the endpoint
//! name and a random per-launch token; the bridge never opens project or world files.
//! Every request is handled by the same `WorldService` used by the UI, so world data,
//! revisions and policy stay authoritative in the Core.

use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{GenericNamespaced, Listener, ListenerOptions, Stream};
use interprocess::TryClone;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use uuid::Uuid;
use world_application::{app_info, AppError, ErrorCode, Query};

use crate::state::{session_status, AppState, LOCAL_PROJECT_ID};

pub const PROTOCOL_VERSION: u32 = 1;
pub const DISCOVERY_SCHEMA_VERSION: u32 = 1;

const MAX_MESSAGE_BYTES: usize = 256 * 1024;
const MAX_REQUEST_ID_BYTES: usize = 128;
const DISCOVERY_DIR: &str = "mcp";
const DISCOVERY_FILE: &str = "runtime.json";

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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct QueryParams {
    project_id: String,
    query: Query,
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
                        if let Err(error) = serve_connection(stream, &token, &state) {
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

fn serve_connection(stream: Stream, expected_token: &str, state: &AppState) -> io::Result<()> {
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
            Some(request) => handle_request(state, expected_token, request),
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

fn handle_request(state: &AppState, expected_token: &str, request: WireRequest) -> WireResponse {
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

    match dispatch(state, &request.method, request.params) {
        Ok(result) => WireResponse::ok(id, result),
        Err(error) => WireResponse::error(id, error),
    }
}

fn dispatch(state: &AppState, method: &str, params: Value) -> Result<Value, AppError> {
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
            }))
        }
        "world_info" => {
            let params: ProjectParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            let service = state
                .service
                .lock()
                .map_err(|_| AppError::internal("world state lock poisoned"))?;
            to_value(service.world_info())
        }
        "query" => {
            let params: QueryParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            let service = state
                .service
                .lock()
                .map_err(|_| AppError::internal("world state lock poisoned"))?;
            to_value(service.query(params.query)?)
        }
        "project_status" => {
            let params: ProjectParams = parse_params(params)?;
            authorize_project(state, &params.project_id)?;
            to_value(session_status(state)?)
        }
        other => Err(AppError::invalid_argument(format!(
            "unknown bridge method `{other}`"
        ))),
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
                serve_connection(connection, "token", &state).expect("serve");
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
