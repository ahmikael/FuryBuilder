//! `bridge_info`: the read-only handshake and diagnostics tool.

use rmcp::model::{CallToolResult, ClientCapabilities};
use serde_json::{json, Map, Value};

use crate::apps::{resolve_apps_support, OBSERVATION_APP_URI, OBSERVATION_TOOL_NAME};
use crate::capabilities::{
    optional_extensions, resolve_tasks_support, BRIDGE_IMPLEMENTS_APPS, BRIDGE_IMPLEMENTS_TASKS,
    BRIDGE_NAME, BRIDGE_VERSION, MODERN_PROTOCOL_VERSION,
};
use crate::context::{evaluate_context, RequestedContext};
use crate::desktop::probe_desktop;
use crate::prompts::workflow_summary_json;

use super::{bridge_error, project_id, text_result, RequestInfo, ToolContext, JOB_TOOL_NAMES};

const TASKS_EXTENSION_ID: &str = "io.modelcontextprotocol/tasks";

fn declares_tasks(capabilities: Option<&ClientCapabilities>) -> bool {
    capabilities
        .and_then(|capabilities| capabilities.extensions.as_ref())
        .is_some_and(|extensions| extensions.contains_key(TASKS_EXTENSION_ID))
}

pub async fn bridge_info(
    args: &Map<String, Value>,
    context: &ToolContext,
    request: &RequestInfo,
) -> CallToolResult {
    let project_id = match project_id(args) {
        Ok(project_id) => project_id,
        Err(error) => return bridge_error(error),
    };
    let world_revision = match crate::args::optional_revision(args, "world_revision") {
        Ok(revision) => revision,
        Err(error) => return bridge_error(error),
    };

    let desktop = probe_desktop(&context.options()).await;
    let active = desktop.active_context();
    let evaluation = evaluate_context(
        RequestedContext {
            project_id: project_id.clone(),
            world_revision,
        },
        active,
    );
    let job_backend_connected = desktop.jobs_available();
    let tasks = resolve_tasks_support(
        declares_tasks(request.client_capabilities.as_ref()),
        job_backend_connected,
        BRIDGE_IMPLEMENTS_TASKS,
    );
    let apps = resolve_apps_support(request.client_capabilities.as_ref(), BRIDGE_IMPLEMENTS_APPS);
    let mut apps_json = apps.to_json();
    if let Some(object) = apps_json.as_object_mut() {
        object.insert(
            "bridgeImplementsApps".to_owned(),
            json!(BRIDGE_IMPLEMENTS_APPS),
        );
        object.insert("resourceUri".to_owned(), json!(OBSERVATION_APP_URI));
        object.insert("tool".to_owned(), json!(OBSERVATION_TOOL_NAME));
        object.insert(
            "fallback".to_owned(),
            json!({
                "tools": [OBSERVATION_TOOL_NAME, "capture_viewport"],
                "content": ["image/png", "application/json"],
            }),
        );
    }
    let mut tasks_json = tasks.to_json();
    if let Some(object) = tasks_json.as_object_mut() {
        object.insert(
            "bridgeImplementsTasks".to_owned(),
            json!(BRIDGE_IMPLEMENTS_TASKS),
        );
    }
    let client_capabilities = request
        .client_capabilities
        .as_ref()
        .map(|capabilities| serde_json::to_value(capabilities).unwrap_or(Value::Null))
        .unwrap_or(Value::Null);

    let info = json!({
        "bridge": {
            "name": BRIDGE_NAME,
            "version": BRIDGE_VERSION,
            "protocolTarget": MODERN_PROTOCOL_VERSION,
            "legacyFallback": true,
            "contextPolicy": "explicit-per-request",
            "optionalExtensions": optional_extensions(),
            "workflow": workflow_summary_json(),
            "apps": apps_json,
            "jobs": {
                "backendConnected": job_backend_connected,
                "fallbackTools": JOB_TOOL_NAMES,
                "tasks": tasks_json
            },
            "desktop": desktop.to_json(),
            "connection": {
                "instanceId": context.client_id,
                "startedAtMs": context.started_at_ms,
                "transport": "stdio",
                "lifecycle": "ephemeral-stdio",
                "expiresWithProcess": true,
                "contextPolicy": "explicit-per-request",
                "state": evaluation.state.as_str()
            },
            "context": evaluation.to_json()
        },
        "request": {
            "projectId": project_id,
            "worldRevision": world_revision,
            "protocolEra": request.era.as_str(),
            "protocolVersion": request.protocol_version,
            "clientCapabilities": client_capabilities
        }
    });
    text_result(&info)
}
