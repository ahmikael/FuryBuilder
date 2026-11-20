//! Authenticated client for the local endpoint owned by the running FuryBuilder desktop
//! process, plus the discovery file that publishes it.
//!
//! The bridge never opens project or world files. Every call reaches the same authoritative
//! Application API as the UI, and a missing or stale endpoint is reported honestly instead
//! of being replaced with fabricated data.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

pub const DESKTOP_PROTOCOL_VERSION: u64 = 1;
pub const DESKTOP_DISCOVERY_SCHEMA_VERSION: u64 = 1;

const MAX_RUNTIME_FILE_BYTES: u64 = 16 * 1024;
/// A PNG viewport capture is base64-encoded inside one JSON response. The Tauri endpoint
/// caps the image at 12 MiB; leave room for metadata and JSON quoting.
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
pub const JOB_START_TIMEOUT: Duration = Duration::from_secs(120);
pub const JOB_QUERY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopUnavailableReason {
    RuntimeFileMissing,
    InvalidRuntimeFile,
    AppNotRunning,
    AuthenticationFailed,
    ProtocolMismatch,
    Timeout,
    InvalidResponse,
    DesktopError,
}

impl DesktopUnavailableReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeFileMissing => "runtime_file_missing",
            Self::InvalidRuntimeFile => "invalid_runtime_file",
            Self::AppNotRunning => "app_not_running",
            Self::AuthenticationFailed => "authentication_failed",
            Self::ProtocolMismatch => "protocol_mismatch",
            Self::Timeout => "timeout",
            Self::InvalidResponse => "invalid_response",
            Self::DesktopError => "desktop_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopError {
    pub reason: DesktopUnavailableReason,
    pub message: String,
    pub code: Option<String>,
}

impl DesktopError {
    pub fn new(reason: DesktopUnavailableReason, message: impl Into<String>) -> Self {
        Self {
            reason,
            message: message.into(),
            code: None,
        }
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Prefix used by tool results. Never includes the token or a local path.
    pub fn tool_text(&self) -> String {
        let code = self.code.as_deref().unwrap_or(self.reason.as_str());
        format!("{code}: {}", self.message)
    }
}

impl fmt::Display for DesktopError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.reason.as_str(), self.message)
    }
}

impl std::error::Error for DesktopError {}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopRuntime {
    pub schema_version: u64,
    pub transport: String,
    pub endpoint: String,
    pub token: String,
    pub pid: u32,
    pub protocol_version: u64,
    pub app_version: String,
    pub started_at_ms: u64,
}

/// Resolves the discovery file without trusting an arbitrary user-provided path.
pub fn runtime_file_path() -> Result<PathBuf, DesktopError> {
    if let Some(path) = std::env::var_os("FURYBUILDER_MCP_RUNTIME") {
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }
    let bundle = "com.furybuilder.desktop";
    #[cfg(windows)]
    {
        let app_data = std::env::var_os("APPDATA").ok_or_else(|| {
            DesktopError::new(
                DesktopUnavailableReason::RuntimeFileMissing,
                "APPDATA is not defined, so the FuryBuilder desktop endpoint cannot be discovered",
            )
        })?;
        return Ok(PathBuf::from(app_data)
            .join(bundle)
            .join("mcp")
            .join("runtime.json"));
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").ok_or_else(|| {
            DesktopError::new(
                DesktopUnavailableReason::RuntimeFileMissing,
                "HOME is not defined, so the FuryBuilder desktop endpoint cannot be discovered",
            )
        })?;
        return Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join(bundle)
            .join("mcp")
            .join("runtime.json"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".local").join("share"))
            })
            .ok_or_else(|| {
                DesktopError::new(
                    DesktopUnavailableReason::RuntimeFileMissing,
                    "Neither XDG_DATA_HOME nor HOME is defined, so the FuryBuilder desktop endpoint cannot be discovered",
                )
            })?;
        return Ok(data_home.join(bundle).join("mcp").join("runtime.json"));
    }
    #[allow(unreachable_code)]
    Err(DesktopError::new(
        DesktopUnavailableReason::RuntimeFileMissing,
        "The FuryBuilder desktop endpoint is not supported on this platform",
    ))
}

pub fn read_desktop_runtime() -> Result<DesktopRuntime, DesktopError> {
    read_desktop_runtime_at(&runtime_file_path()?)
}

pub fn read_desktop_runtime_at(path: &Path) -> Result<DesktopRuntime, DesktopError> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(DesktopError::new(
                DesktopUnavailableReason::RuntimeFileMissing,
                "FuryBuilder is not running: start the desktop app to enable MCP tools",
            ));
        }
        Err(error) => {
            return Err(DesktopError::new(
                DesktopUnavailableReason::RuntimeFileMissing,
                format!(
                    "The FuryBuilder desktop endpoint could not be read ({})",
                    error.kind()
                ),
            ));
        }
    };
    if metadata.len() > MAX_RUNTIME_FILE_BYTES {
        return Err(DesktopError::new(
            DesktopUnavailableReason::InvalidRuntimeFile,
            "The FuryBuilder desktop endpoint file is larger than expected",
        ));
    }
    let bytes = std::fs::read(path).map_err(|error| {
        DesktopError::new(
            DesktopUnavailableReason::RuntimeFileMissing,
            format!(
                "The FuryBuilder desktop endpoint could not be read ({})",
                error.kind()
            ),
        )
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        DesktopError::new(
            DesktopUnavailableReason::InvalidRuntimeFile,
            "The FuryBuilder desktop endpoint file is not valid JSON; restart the app",
        )
    })?;
    validate_runtime(&value)
}

fn validate_runtime(value: &Value) -> Result<DesktopRuntime, DesktopError> {
    let Some(record) = value.as_object() else {
        return Err(invalid_runtime("has no object payload"));
    };
    let number = |key: &str| record.get(key).and_then(Value::as_u64);
    let string = |key: &str| record.get(key).and_then(Value::as_str);
    let valid = number("schemaVersion") == Some(DESKTOP_DISCOVERY_SCHEMA_VERSION)
        && number("protocolVersion") == Some(DESKTOP_PROTOCOL_VERSION)
        && string("transport") == Some("local_socket")
        && string("endpoint").is_some_and(|endpoint| !endpoint.is_empty() && endpoint.len() <= 512)
        && string("token").is_some_and(|token| (32..=256).contains(&token.len()))
        && number("pid").is_some_and(|pid| (1..=u64::from(u32::MAX)).contains(&pid))
        && string("appVersion").is_some()
        && number("startedAtMs").is_some();
    if !valid {
        return Err(invalid_runtime(
            "does not match the supported schema; restart the app",
        ));
    }
    Ok(DesktopRuntime {
        schema_version: number("schemaVersion").expect("validated"),
        transport: string("transport").expect("validated").to_owned(),
        endpoint: string("endpoint").expect("validated").to_owned(),
        token: string("token").expect("validated").to_owned(),
        pid: number("pid").expect("validated") as u32,
        protocol_version: number("protocolVersion").expect("validated"),
        app_version: string("appVersion").expect("validated").to_owned(),
        started_at_ms: number("startedAtMs").expect("validated"),
    })
}

fn invalid_runtime(detail: &str) -> DesktopError {
    DesktopError::new(
        DesktopUnavailableReason::InvalidRuntimeFile,
        format!("The FuryBuilder desktop endpoint file {detail}"),
    )
}

#[derive(Debug, Clone)]
pub struct CallOptions {
    pub client_id: String,
    pub runtime_path: Option<PathBuf>,
    pub runtime: Option<DesktopRuntime>,
    pub timeout: Duration,
}

impl CallOptions {
    pub fn new(client_id: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            runtime_path: None,
            runtime: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_runtime_path(mut self, path: PathBuf) -> Self {
        self.runtime_path = Some(path);
        self
    }

    pub fn with_runtime(mut self, runtime: DesktopRuntime) -> Self {
        self.runtime = Some(runtime);
        self
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireResponse {
    protocol: u64,
    id: String,
    ok: bool,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<WireError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireError {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// Sends one request over a fresh local connection and awaits exactly one response.
pub async fn call_desktop(
    method: &str,
    params: Value,
    options: &CallOptions,
) -> Result<Value, DesktopError> {
    let runtime = match &options.runtime {
        Some(runtime) => runtime.clone(),
        None => match &options.runtime_path {
            Some(path) => read_desktop_runtime_at(path)?,
            None => read_desktop_runtime()?,
        },
    };
    let id = uuid::Uuid::new_v4().to_string();
    let request = json!({
        "protocol": DESKTOP_PROTOCOL_VERSION,
        "id": id,
        "token": runtime.token,
        "clientId": options.client_id,
        "method": method,
        "params": params,
    });
    let mut payload = serde_json::to_vec(&request).map_err(|error| {
        DesktopError::new(DesktopUnavailableReason::DesktopError, error.to_string())
    })?;
    payload.push(b'\n');

    let exchange = async {
        let mut stream = connect(&runtime.endpoint).await?;
        stream.write_all(&payload).await?;
        stream.flush().await?;
        let line = read_message(&mut stream).await?;
        Ok::<Vec<u8>, io::Error>(line)
    };

    let line = match tokio::time::timeout(options.timeout, exchange).await {
        Ok(Ok(line)) => line,
        Ok(Err(error)) => return Err(map_io_error(&error)),
        Err(_) => {
            return Err(DesktopError::new(
                DesktopUnavailableReason::Timeout,
                format!(
                    "FuryBuilder did not answer within {} ms",
                    options.timeout.as_millis()
                ),
            ));
        }
    };

    let response: WireResponse = serde_json::from_slice(&line).map_err(|_| {
        DesktopError::new(
            DesktopUnavailableReason::InvalidResponse,
            "FuryBuilder returned malformed JSON",
        )
    })?;
    if response.protocol != DESKTOP_PROTOCOL_VERSION {
        return Err(DesktopError::new(
            DesktopUnavailableReason::ProtocolMismatch,
            "The desktop app and MCP bridge protocol versions do not match",
        ));
    }
    if response.id != id || response.ok && response.result.is_none() {
        return Err(DesktopError::new(
            DesktopUnavailableReason::InvalidResponse,
            "FuryBuilder returned an unexpected bridge response",
        ));
    }
    if !response.ok {
        let code = response.error.as_ref().and_then(|error| error.code.clone());
        let message = response
            .error
            .and_then(|error| error.message)
            .unwrap_or_else(|| "FuryBuilder rejected the bridge request".to_owned());
        let reason = match code.as_deref() {
            Some("unauthorized") => DesktopUnavailableReason::AuthenticationFailed,
            Some("unsupported_version") => DesktopUnavailableReason::ProtocolMismatch,
            _ => DesktopUnavailableReason::DesktopError,
        };
        let message = match reason {
            DesktopUnavailableReason::AuthenticationFailed => {
                "The FuryBuilder desktop endpoint rejected the bridge token; restart both"
                    .to_owned()
            }
            DesktopUnavailableReason::ProtocolMismatch => {
                "The desktop app and MCP bridge protocol versions do not match".to_owned()
            }
            _ => message,
        };
        let mut error = DesktopError::new(reason, message);
        error.code = code;
        return Err(error);
    }
    Ok(response.result.expect("checked above"))
}

fn map_io_error(error: &io::Error) -> DesktopError {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused | io::ErrorKind::WouldBlock => {
            DesktopError::new(
                DesktopUnavailableReason::AppNotRunning,
                "FuryBuilder is not running or its local endpoint has closed; start the desktop app",
            )
            .with_code(if error.kind() == io::ErrorKind::NotFound {
                "ENOENT"
            } else {
                "ECONNREFUSED"
            })
        }
        io::ErrorKind::PermissionDenied => DesktopError::new(
            DesktopUnavailableReason::AuthenticationFailed,
            "Windows refused access to the FuryBuilder local endpoint; restart both processes",
        ),
        _ if error.to_string().contains("oversized") => DesktopError::new(
            DesktopUnavailableReason::InvalidResponse,
            "FuryBuilder returned an oversized response",
        ),
        io::ErrorKind::UnexpectedEof => DesktopError::new(
            DesktopUnavailableReason::InvalidResponse,
            "FuryBuilder closed the connection without a response",
        ),
        _ => DesktopError::new(
            DesktopUnavailableReason::DesktopError,
            format!(
                "The FuryBuilder desktop endpoint failed ({})",
                error.kind()
            ),
        ),
    }
}

async fn read_message<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut line = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        if line.len() + read > MAX_RESPONSE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "FuryBuilder returned an oversized response",
            ));
        }
        line.extend_from_slice(&chunk[..read]);
        if let Some(newline) = line.iter().position(|byte| *byte == b'\n') {
            line.truncate(newline);
            break;
        }
    }
    if line.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "FuryBuilder closed the connection without a response",
        ));
    }
    Ok(line)
}

#[cfg(windows)]
async fn connect(endpoint: &str) -> io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    // ERROR_PIPE_BUSY: a server instance exists but is not accepting yet. A correct
    // client retries briefly instead of reporting the desktop as missing.
    const ERROR_PIPE_BUSY: i32 = 231;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        match tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint) {
            Ok(client) => return Ok(client),
            Err(error)
                if error.raw_os_error() == Some(ERROR_PIPE_BUSY)
                    && tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(unix)]
async fn connect(endpoint: &str) -> io::Result<tokio::net::UnixStream> {
    tokio::net::UnixStream::connect(endpoint).await
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeProject {
    pub project_id: String,
    pub name: Option<String>,
    pub world_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeBridge {
    pub protocol_version: u64,
    pub transport: String,
    pub pid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeAccess {
    pub mode: String,
    pub writes_enabled: bool,
    pub max_blocks: u64,
    pub max_batch_changes: u64,
    pub require_confirmation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeRenderer {
    pub listening: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobsSummary {
    pub available: bool,
    pub queued: u64,
    pub running: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub cancelled: u64,
    pub retained: u64,
    pub max_queued: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedProbe {
    pub connected: bool,
    pub app: Value,
    pub project: ProbeProject,
    pub bridge: ProbeBridge,
    pub access: Option<ProbeAccess>,
    pub renderer: Option<ProbeRenderer>,
    pub jobs: Option<JobsSummary>,
    pub pending_confirmations: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    Connected(Box<ConnectedProbe>),
    Disconnected {
        reason: DesktopUnavailableReason,
        message: String,
    },
}

impl Probe {
    pub fn to_json(&self) -> Value {
        match self {
            Self::Connected(connected) => {
                serde_json::to_value(connected).expect("connected probe is serializable")
            }
            Self::Disconnected { reason, message } => json!({
                "connected": false,
                "reason": reason.as_str(),
                "message": message,
            }),
        }
    }

    pub fn active_context(&self) -> Option<crate::context::ActiveContext> {
        match self {
            Self::Connected(connected) => Some(crate::context::ActiveContext {
                project_id: connected.project.project_id.clone(),
                world_revision: connected.project.world_revision,
            }),
            Self::Disconnected { .. } => None,
        }
    }

    pub fn jobs_available(&self) -> bool {
        matches!(
            self,
            Self::Connected(connected) if connected.jobs.as_ref().is_some_and(|jobs| jobs.available)
        )
    }
}

/// Non-failing diagnostic used by `bridge_info`; no token or local path is exposed.
pub async fn probe_desktop(options: &CallOptions) -> Probe {
    let options = options.clone().with_timeout(PROBE_TIMEOUT);
    let result = match call_desktop("ping", json!({}), &options).await {
        Ok(result) => result,
        Err(error) => {
            return Probe::Disconnected {
                reason: error.reason,
                message: error.message,
            };
        }
    };
    let Some(result) = result.as_object() else {
        return disconnected_invalid("FuryBuilder returned an incomplete ping response");
    };
    let project = result.get("project").and_then(Value::as_object);
    let bridge = result.get("bridge").and_then(Value::as_object);
    let (Some(project), Some(bridge)) = (project, bridge) else {
        return disconnected_invalid("FuryBuilder returned an incomplete ping response");
    };
    let (Some(project_id), Some(world_revision), Some(protocol_version), Some(pid)) = (
        project.get("projectId").and_then(Value::as_str),
        project.get("worldRevision").and_then(Value::as_u64),
        bridge.get("protocolVersion").and_then(Value::as_u64),
        bridge.get("pid").and_then(Value::as_u64),
    ) else {
        return disconnected_invalid("FuryBuilder returned an incomplete ping response");
    };

    let access = result
        .get("mcp")
        .and_then(Value::as_object)
        .and_then(|mcp| {
            Some(ProbeAccess {
                mode: mcp.get("mode")?.as_str()?.to_owned(),
                writes_enabled: mcp.get("writesEnabled")?.as_bool()?,
                max_blocks: mcp.get("maxBlocks")?.as_u64()?,
                max_batch_changes: mcp.get("maxBatchChanges")?.as_u64()?,
                require_confirmation: mcp.get("requireConfirmation")?.as_bool()?,
            })
        });
    let renderer = result
        .get("renderer")
        .and_then(Value::as_object)
        .and_then(|renderer| {
            Some(ProbeRenderer {
                listening: renderer.get("listening")?.as_bool()?,
            })
        });
    let jobs = result
        .get("jobs")
        .and_then(|jobs| serde_json::from_value(jobs.clone()).ok());

    Probe::Connected(Box::new(ConnectedProbe {
        connected: true,
        app: result.get("app").cloned().unwrap_or(Value::Null),
        project: ProbeProject {
            project_id: project_id.to_owned(),
            name: project
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_owned),
            world_revision,
        },
        bridge: ProbeBridge {
            protocol_version,
            transport: bridge
                .get("transport")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            pid: pid as u32,
        },
        access,
        renderer,
        jobs,
        pending_confirmations: result
            .get("pendingConfirmations")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    }))
}

fn disconnected_invalid(message: &str) -> Probe {
    Probe::Disconnected {
        reason: DesktopUnavailableReason::InvalidResponse,
        message: message.to_owned(),
    }
}

#[derive(Debug, Clone)]
pub struct StartJobRequest {
    pub project_id: String,
    pub kind: String,
    pub parameters: Map<String, Value>,
    pub idempotency_key: String,
    pub expected_revision: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobError {
    Unavailable(String),
    Failed { code: String, message: String },
}

impl JobError {
    pub fn tool_text(&self) -> String {
        match self {
            Self::Unavailable(message) => format!("job_api_unavailable: {message}"),
            Self::Failed { code, message } => format!("{code}: {message}"),
        }
    }
}

fn map_desktop_job_error(error: DesktopError) -> JobError {
    match error.reason {
        DesktopUnavailableReason::RuntimeFileMissing
        | DesktopUnavailableReason::InvalidRuntimeFile
        | DesktopUnavailableReason::AppNotRunning
        | DesktopUnavailableReason::AuthenticationFailed
        | DesktopUnavailableReason::ProtocolMismatch
        | DesktopUnavailableReason::Timeout
        | DesktopUnavailableReason::InvalidResponse => JobError::Unavailable(format!(
            "The FuryBuilder desktop Job API is unavailable ({}): {}",
            error.reason.as_str(),
            error.message
        )),
        DesktopUnavailableReason::DesktopError => JobError::Failed {
            code: error.code.unwrap_or_else(|| "desktop_error".to_owned()),
            message: error.message,
        },
    }
}

fn validate_job_snapshot(value: &Value) -> Result<Value, JobError> {
    let Some(snapshot) = value.as_object() else {
        return Err(JobError::Failed {
            code: "invalid_response".to_owned(),
            message: "FuryBuilder returned no job snapshot".to_owned(),
        });
    };
    let valid_state = snapshot
        .get("state")
        .and_then(Value::as_str)
        .is_some_and(|state| {
            matches!(
                state,
                "queued" | "running" | "succeeded" | "failed" | "cancelled"
            )
        });
    let valid = snapshot.get("jobId").and_then(Value::as_str).is_some()
        && snapshot.get("projectId").and_then(Value::as_str).is_some()
        && valid_state
        && snapshot
            .get("progress")
            .and_then(Value::as_f64)
            .is_some_and(f64::is_finite);
    if !valid {
        return Err(JobError::Failed {
            code: "invalid_response".to_owned(),
            message: "FuryBuilder returned an incomplete job snapshot".to_owned(),
        });
    }
    Ok(value.clone())
}

async fn call_job(
    method: &str,
    params: Value,
    options: &CallOptions,
    timeout: Duration,
) -> Result<Value, JobError> {
    let options = options.clone().with_timeout(timeout);
    match call_desktop(method, params, &options).await {
        Ok(value) => validate_job_snapshot(&value),
        Err(error) => Err(map_desktop_job_error(error)),
    }
}

pub async fn start_job(
    request: &StartJobRequest,
    options: &CallOptions,
) -> Result<Value, JobError> {
    let mut params = json!({
        "projectId": request.project_id,
        "kind": request.kind,
        "parameters": Value::Object(request.parameters.clone()),
        "idempotencyKey": request.idempotency_key,
    });
    if let Some(revision) = request.expected_revision {
        params["worldRevision"] = json!(revision);
    }
    call_job("start_job", params, options, JOB_START_TIMEOUT).await
}

pub async fn job_status(
    project_id: &str,
    job_id: &str,
    options: &CallOptions,
) -> Result<Value, JobError> {
    call_job(
        "job_status",
        json!({ "projectId": project_id, "jobId": job_id }),
        options,
        JOB_QUERY_TIMEOUT,
    )
    .await
}

pub async fn cancel_job(
    project_id: &str,
    job_id: &str,
    options: &CallOptions,
) -> Result<Value, JobError> {
    call_job(
        "cancel_job",
        json!({ "projectId": project_id, "jobId": job_id }),
        options,
        JOB_QUERY_TIMEOUT,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime_json() -> Value {
        json!({
            "schemaVersion": 1,
            "transport": "local_socket",
            "endpoint": "furybuilder-test",
            "token": "x".repeat(32),
            "pid": 42,
            "protocolVersion": 1,
            "appVersion": "0.1.0",
            "startedAtMs": 1
        })
    }

    #[test]
    fn validates_the_discovery_document() {
        let runtime = validate_runtime(&runtime_json()).expect("valid runtime");
        assert_eq!(runtime.pid, 42);
        assert_eq!(runtime.token.len(), 32);

        let mut short_token = runtime_json();
        short_token["token"] = json!("short");
        assert_eq!(
            validate_runtime(&short_token)
                .expect_err("short token")
                .reason,
            DesktopUnavailableReason::InvalidRuntimeFile
        );

        let mut wrong_transport = runtime_json();
        wrong_transport["transport"] = json!("tcp");
        assert!(validate_runtime(&wrong_transport).is_err());
    }

    #[test]
    fn reads_and_validates_the_discovery_file() {
        let root = std::env::temp_dir().join(format!(
            "furybuilder-mcp-discovery-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).expect("dir");
        let path = root.join("runtime.json");
        std::fs::write(&path, serde_json::to_vec(&runtime_json()).expect("json")).expect("write");
        let runtime = read_desktop_runtime_at(&path).expect("runtime");
        assert_eq!(runtime.app_version, "0.1.0");

        std::fs::write(&path, b"not json").expect("write");
        assert_eq!(
            read_desktop_runtime_at(&path).expect_err("invalid").reason,
            DesktopUnavailableReason::InvalidRuntimeFile
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_discovery_file_is_an_honest_unavailable_result() {
        let path = std::env::temp_dir().join(format!(
            "furybuilder-mcp-missing-{}.json",
            uuid::Uuid::new_v4().simple()
        ));
        let error = read_desktop_runtime_at(&path).expect_err("missing");
        assert_eq!(error.reason, DesktopUnavailableReason::RuntimeFileMissing);
        assert!(error
            .tool_text()
            .starts_with("runtime_file_missing: FuryBuilder is not running"));
    }

    #[test]
    fn job_snapshots_are_validated_instead_of_invented() {
        let valid = json!({
            "jobId": "job-1",
            "projectId": "p",
            "state": "running",
            "progress": 42
        });
        assert!(validate_job_snapshot(&valid).is_ok());
        let missing_progress = json!({
            "jobId": "job-1",
            "projectId": "p",
            "state": "running"
        });
        assert!(matches!(
            validate_job_snapshot(&missing_progress),
            Err(JobError::Failed { code, .. }) if code == "invalid_response"
        ));
    }
}
