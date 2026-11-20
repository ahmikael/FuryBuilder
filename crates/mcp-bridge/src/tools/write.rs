//! Safe write tools.
//!
//! Preview never mutates the world. Apply and undo require an explicit user grant in the
//! running editor; without it the authoritative endpoint refuses the request. The bridge
//! cannot grant itself write access.

use std::time::Duration;

use rmcp::model::CallToolResult;
use serde_json::{json, Map, Value};

use crate::args;

use super::{call_desktop, desktop_error, project_id, text_result, with_revision, ToolContext};

const PREVIEW_TIMEOUT: Duration = Duration::from_secs(15);
const WRITE_TIMEOUT: Duration = Duration::from_secs(120);

async fn desktop_call(
    context: &ToolContext,
    method: &str,
    params: Value,
    timeout: Duration,
) -> CallToolResult {
    match call_desktop(context, method, params, Some(timeout)).await {
        Ok(result) => text_result(&result),
        Err(error) => desktop_error(error),
    }
}

fn operation(args: &Map<String, Value>) -> Result<Value, String> {
    let value = args
        .get("operation")
        .ok_or_else(|| "operation is required".to_owned())?;
    args::normalize_operation(value)
}

fn revision(args: &Map<String, Value>) -> Result<u64, String> {
    args::required_revision(args, "world_revision")
}

pub async fn call(name: &str, args: &Map<String, Value>, context: &ToolContext) -> CallToolResult {
    match name {
        "preview_operation" => preview_operation(args, context).await,
        "apply_operation" => apply_operation(args, context).await,
        "apply_operations_batch" => apply_operations_batch(args, context).await,
        "insert_template" => insert_template(args, context).await,
        "undo_operation" => undo_operation(args, context).await,
        "get_operation_status" => get_operation_status(args, context).await,
        other => super::bridge_error(format!("unknown write tool `{other}`")),
    }
}

async fn preview_operation(args: &Map<String, Value>, context: &ToolContext) -> CallToolResult {
    let project = match project_id(args) {
        Ok(project) => project,
        Err(error) => return super::bridge_error(error),
    };
    let revision = match args::optional_revision(args, "world_revision") {
        Ok(revision) => revision,
        Err(error) => return super::bridge_error(error),
    };
    let operation = match operation(args) {
        Ok(operation) => operation,
        Err(error) => return super::bridge_error(error),
    };
    let params = with_revision(
        json!({ "projectId": project, "operation": operation }),
        revision,
    );
    desktop_call(context, "preview_operation", params, PREVIEW_TIMEOUT).await
}

async fn apply_operation(args: &Map<String, Value>, context: &ToolContext) -> CallToolResult {
    let project = match project_id(args) {
        Ok(project) => project,
        Err(error) => return super::bridge_error(error),
    };
    let revision = match revision(args) {
        Ok(revision) => revision,
        Err(error) => return super::bridge_error(error),
    };
    let idempotency_key = match args::required_string(args, "idempotency_key", 1, 128) {
        Ok(key) => key,
        Err(error) => return super::bridge_error(error),
    };
    let operation = match operation(args) {
        Ok(operation) => operation,
        Err(error) => return super::bridge_error(error),
    };
    let params = json!({
        "projectId": project,
        "worldRevision": revision,
        "operation": operation,
        "idempotencyKey": idempotency_key,
    });
    desktop_call(context, "apply_operation", params, WRITE_TIMEOUT).await
}

async fn apply_operations_batch(
    args: &Map<String, Value>,
    context: &ToolContext,
) -> CallToolResult {
    let project = match project_id(args) {
        Ok(project) => project,
        Err(error) => return super::bridge_error(error),
    };
    let revision = match revision(args) {
        Ok(revision) => revision,
        Err(error) => return super::bridge_error(error),
    };
    let dimension = match args::required_string(args, "dimension", 1, 128) {
        Ok(dimension) => dimension,
        Err(error) => return super::bridge_error(error),
    };
    let changes = match args::required_array(args, "changes") {
        Ok(changes) => changes.clone(),
        Err(error) => return super::bridge_error(error),
    };
    let idempotency_key = match args::required_string(args, "idempotency_key", 1, 128) {
        Ok(key) => key,
        Err(error) => return super::bridge_error(error),
    };
    let operation = match args::normalize_operation(&json!({
        "type": "batch",
        "dimension": dimension,
        "changes": changes,
    })) {
        Ok(operation) => operation,
        Err(error) => return super::bridge_error(error),
    };
    let params = json!({
        "projectId": project,
        "worldRevision": revision,
        "operation": operation,
        "idempotencyKey": idempotency_key,
    });
    desktop_call(context, "apply_operation", params, WRITE_TIMEOUT).await
}

async fn insert_template(args: &Map<String, Value>, context: &ToolContext) -> CallToolResult {
    let project = match project_id(args) {
        Ok(project) => project,
        Err(error) => return super::bridge_error(error),
    };
    let revision = match revision(args) {
        Ok(revision) => revision,
        Err(error) => return super::bridge_error(error),
    };
    let template_id = match args::required_string(args, "template_id", 1, 128) {
        Ok(template_id) => template_id,
        Err(error) => return super::bridge_error(error),
    };
    if uuid::Uuid::parse_str(&template_id).is_err() {
        return super::bridge_error("template_id must be a UUID");
    }
    let dimension = match args::required_string(args, "dimension", 1, 128) {
        Ok(dimension) => dimension,
        Err(error) => return super::bridge_error(error),
    };
    let origin = match args::required_array(args, "origin") {
        Ok(origin) if origin.len() == 3 => origin,
        Ok(_) => return super::bridge_error("origin must contain exactly 3 integers"),
        Err(error) => return super::bridge_error(error),
    };
    let origin = match origin
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value
                .as_i64()
                .ok_or_else(|| format!("origin[{index}] must be an integer"))
        })
        .collect::<Result<Vec<i64>, String>>()
    {
        Ok(origin) => origin,
        Err(error) => return super::bridge_error(error),
    };
    let include_air = match args::optional_bool(args, "include_air", false) {
        Ok(include_air) => include_air,
        Err(error) => return super::bridge_error(error),
    };
    let layer_id = match args::optional_string(args, "layer_id", 1, 64) {
        Ok(layer_id) => layer_id,
        Err(error) => return super::bridge_error(error),
    };
    let idempotency_key = match args::required_string(args, "idempotency_key", 1, 128) {
        Ok(key) => key,
        Err(error) => return super::bridge_error(error),
    };
    let mut payload = json!({
        "projectId": project,
        "worldRevision": revision,
        "templateId": template_id,
        "dimension": dimension,
        "origin": origin,
        "includeAir": include_air,
        "idempotencyKey": idempotency_key,
    });
    if let Some(layer_id) = layer_id {
        payload["layerId"] = json!(layer_id);
    }
    desktop_call(context, "insert_template", payload, WRITE_TIMEOUT).await
}

async fn undo_operation(args: &Map<String, Value>, context: &ToolContext) -> CallToolResult {
    let project = match project_id(args) {
        Ok(project) => project,
        Err(error) => return super::bridge_error(error),
    };
    let revision = match revision(args) {
        Ok(revision) => revision,
        Err(error) => return super::bridge_error(error),
    };
    let operation_id = match args::optional_string(args, "operation_id", 1, 128) {
        Ok(operation_id) => operation_id,
        Err(error) => return super::bridge_error(error),
    };
    let mut payload = json!({ "projectId": project, "worldRevision": revision });
    if let Some(operation_id) = operation_id {
        payload["operationId"] = json!(operation_id);
    }
    desktop_call(context, "undo_operation", payload, WRITE_TIMEOUT).await
}

async fn get_operation_status(args: &Map<String, Value>, context: &ToolContext) -> CallToolResult {
    let project = match project_id(args) {
        Ok(project) => project,
        Err(error) => return super::bridge_error(error),
    };
    let operation_id = match args::optional_string(args, "operation_id", 1, 128) {
        Ok(operation_id) => operation_id,
        Err(error) => return super::bridge_error(error),
    };
    let mut payload = json!({ "projectId": project });
    if let Some(operation_id) = operation_id {
        payload["operationId"] = json!(operation_id);
    }
    desktop_call(context, "operation_status", payload, Duration::from_secs(5)).await
}
