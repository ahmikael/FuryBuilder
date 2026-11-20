//! Read-only MCP tools over the authoritative Application API.
//!
//! None of them mutates the world or the project; screenshots are real viewport frames,
//! never generated text.

use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::args;

use super::{call_desktop, desktop_error, project_id, text_result, with_revision, ToolContext};

const PNG_DATA_URL_PREFIX: &str = "data:image/png;base64,";
/// The Tauri endpoint waits up to 10 s for a mounted viewport; the socket client must
/// outlive that window instead of reporting a false timeout.
const RENDER_TIMEOUT: Duration = Duration::from_secs(15);

async fn desktop_call(
    context: &ToolContext,
    method: &str,
    params: Value,
    timeout: Option<Duration>,
) -> Result<Value, CallToolResult> {
    call_desktop(context, method, params, timeout)
        .await
        .map_err(desktop_error)
}

fn revision(args: &Map<String, Value>) -> Result<Option<u64>, String> {
    args::optional_revision(args, "world_revision")
}

async fn query_tool(
    args: &Map<String, Value>,
    context: &ToolContext,
    query: Value,
) -> CallToolResult {
    let project = match project_id(args) {
        Ok(project) => project,
        Err(error) => return super::bridge_error(error),
    };
    let revision = match revision(args) {
        Ok(revision) => revision,
        Err(error) => return super::bridge_error(error),
    };
    let params = with_revision(json!({ "projectId": project, "query": query }), revision);
    match desktop_call(context, "query", params, None).await {
        Ok(result) => text_result(&result),
        Err(error) => error,
    }
}

async fn simple_call(
    args: &Map<String, Value>,
    context: &ToolContext,
    method: &str,
) -> CallToolResult {
    let project = match project_id(args) {
        Ok(project) => project,
        Err(error) => return super::bridge_error(error),
    };
    let revision = match revision(args) {
        Ok(revision) => revision,
        Err(error) => return super::bridge_error(error),
    };
    let params = with_revision(json!({ "projectId": project }), revision);
    match desktop_call(context, method, params, None).await {
        Ok(result) => text_result(&result),
        Err(error) => error,
    }
}

pub async fn call(name: &str, args: &Map<String, Value>, context: &ToolContext) -> CallToolResult {
    match name {
        "world_info" => simple_call(args, context, "world_info").await,
        "list_templates" => simple_call(args, context, "list_templates").await,
        "get_workspace_policy" => {
            query_tool(args, context, json!({ "type": "workspace_policy" })).await
        }
        "list_scene_objects" => query_tool(args, context, json!({ "type": "scene_objects" })).await,
        "inspect_scene_object" => inspect_object(args, context, "inspect_scene_object").await,
        "list_parametric_objects" => {
            query_tool(args, context, json!({ "type": "parametric_objects" })).await
        }
        "inspect_parametric_object" => {
            inspect_object(args, context, "inspect_parametric_object").await
        }
        "preview_build_dsl" => preview_build_dsl(args, context).await,
        "get_block" => position_query(args, context, "get_block").await,
        "get_biome" => position_query(args, context, "get_biome").await,
        "find_structure_sites" => find_structure_sites(args, context).await,
        "inspect_region" => region_query(args, context, "region_summary").await,
        "get_region" => region_query(args, context, "region_blocks").await,
        "get_camera" => {
            let project = match project_id(args) {
                Ok(project) => project,
                Err(error) => return super::bridge_error(error),
            };
            let revision = match revision(args) {
                Ok(revision) => revision,
                Err(error) => return super::bridge_error(error),
            };
            let params = with_revision(json!({ "projectId": project }), revision);
            match desktop_call(context, "get_camera", params, Some(RENDER_TIMEOUT)).await {
                Ok(result) => text_result(&result),
                Err(error) => error,
            }
        }
        "observe" => observe(args, context).await,
        "validate_region" => validate_region(args, context).await,
        "capture_viewport" => capture_viewport(args, context).await,
        other => super::bridge_error(format!("unknown read tool `{other}`")),
    }
}

async fn inspect_object(
    args: &Map<String, Value>,
    context: &ToolContext,
    query_type: &str,
) -> CallToolResult {
    let object_id = match args::required_string(args, "object_id", 1, 64) {
        Ok(object_id) => object_id,
        Err(error) => return super::bridge_error(error),
    };
    query_tool(
        args,
        context,
        json!({ "type": query_type, "objectId": object_id }),
    )
    .await
}

async fn position_query(
    args: &Map<String, Value>,
    context: &ToolContext,
    query_type: &str,
) -> CallToolResult {
    let dimension = match args::required_string(args, "dimension", 1, 128) {
        Ok(dimension) => dimension,
        Err(error) => return super::bridge_error(error),
    };
    let position = match args::required_object(args, "pos")
        .and_then(|position| args::block_pos(&Value::Object(position.clone()), "pos"))
    {
        Ok(position) => position,
        Err(error) => return super::bridge_error(error),
    };
    query_tool(
        args,
        context,
        json!({ "type": query_type, "dimension": dimension, "pos": position }),
    )
    .await
}

async fn region_query(
    args: &Map<String, Value>,
    context: &ToolContext,
    query_type: &str,
) -> CallToolResult {
    let dimension = match args::required_string(args, "dimension", 1, 128) {
        Ok(dimension) => dimension,
        Err(error) => return super::bridge_error(error),
    };
    let region = match args::required_object(args, "bounds")
        .and_then(|bounds| args::bounds(&Value::Object(bounds.clone()), "bounds"))
    {
        Ok(region) => region,
        Err(error) => return super::bridge_error(error),
    };
    query_tool(
        args,
        context,
        json!({ "type": query_type, "dimension": dimension, "bounds": region }),
    )
    .await
}

async fn find_structure_sites(args: &Map<String, Value>, context: &ToolContext) -> CallToolResult {
    let dimension = match args::required_string(args, "dimension", 1, 128) {
        Ok(dimension) => dimension,
        Err(error) => return super::bridge_error(error),
    };
    let region = match args::required_object(args, "bounds")
        .and_then(|bounds| args::bounds(&Value::Object(bounds.clone()), "bounds"))
    {
        Ok(region) => region,
        Err(error) => return super::bridge_error(error),
    };
    let width = match args::required_u64(args, "width", 1, 32) {
        Ok(width) => width,
        Err(error) => return super::bridge_error(error),
    };
    let depth = match args::required_u64(args, "depth", 1, 32) {
        Ok(depth) => depth,
        Err(error) => return super::bridge_error(error),
    };
    let step = match args::optional_u64(args, "step", 1, 64, 5) {
        Ok(step) => step,
        Err(error) => return super::bridge_error(error),
    };
    let max_slope = match args::optional_u64(args, "maxSlope", 0, 32, 2) {
        Ok(value) => value,
        Err(error) => return super::bridge_error(error),
    };
    let clearance = match args::optional_u64(args, "clearance", 0, 32, 2) {
        Ok(value) => value,
        Err(error) => return super::bridge_error(error),
    };
    let limit = match args::optional_u64(args, "limit", 1, 64, 8) {
        Ok(limit) => limit,
        Err(error) => return super::bridge_error(error),
    };
    query_tool(
        args,
        context,
        json!({
            "type": "find_structure_sites",
            "dimension": dimension,
            "bounds": region,
            "width": width,
            "depth": depth,
            "step": step,
            "maxSlope": max_slope,
            "clearance": clearance,
            "limit": limit,
        }),
    )
    .await
}

async fn preview_build_dsl(args: &Map<String, Value>, context: &ToolContext) -> CallToolResult {
    let project = match project_id(args) {
        Ok(project) => project,
        Err(error) => return super::bridge_error(error),
    };
    let revision = match revision(args) {
        Ok(revision) => revision,
        Err(error) => return super::bridge_error(error),
    };
    let recipe = match args
        .get("recipe")
        .ok_or_else(|| "recipe is required".to_owned())
        .and_then(args::normalize_build_recipe)
    {
        Ok(recipe) => recipe,
        Err(error) => return super::bridge_error(error),
    };
    let params = with_revision(json!({ "projectId": project, "recipe": recipe }), revision);
    match desktop_call(context, "build_dsl_preview", params, None).await {
        Ok(result) => text_result(&result),
        Err(error) => error,
    }
}

fn png_bytes(data_url: &str) -> Result<Vec<u8>, String> {
    let Some(base64) = data_url.strip_prefix(PNG_DATA_URL_PREFIX) else {
        return Err("the desktop endpoint did not return a PNG payload".to_owned());
    };
    BASE64
        .decode(base64)
        .map_err(|_| "the desktop endpoint returned invalid base64 PNG data".to_owned())
}

fn image_metadata(bytes: &[u8]) -> Value {
    json!({
        "mimeType": "image/png",
        "bytes": bytes.len(),
        "sha256": format!("{:x}", Sha256::digest(bytes)),
    })
}

async fn observe(args: &Map<String, Value>, context: &ToolContext) -> CallToolResult {
    let project = match project_id(args) {
        Ok(project) => project,
        Err(error) => return super::bridge_error(error),
    };
    let revision = match revision(args) {
        Ok(revision) => revision,
        Err(error) => return super::bridge_error(error),
    };
    let observation = match args::normalize_observation(args) {
        Ok(observation) => observation,
        Err(error) => return super::bridge_error(error),
    };
    let params = with_revision(
        json!({ "projectId": project, "observation": observation }),
        revision,
    );
    let result = match desktop_call(context, "observe", params, Some(RENDER_TIMEOUT)).await {
        Ok(result) => result,
        Err(error) => return error,
    };
    let Some(result_object) = result.as_object() else {
        return invalid_response("the desktop endpoint did not return an observation");
    };
    let Some(image) = result_object.get("image").and_then(Value::as_str) else {
        return invalid_response("the desktop endpoint did not return a PNG observation");
    };
    let bytes = match png_bytes(image) {
        Ok(bytes) => bytes,
        Err(error) => return invalid_response(error),
    };
    let base64 = image
        .strip_prefix(PNG_DATA_URL_PREFIX)
        .expect("png_bytes validated the prefix");
    let mut metadata = result_object.clone();
    metadata.remove("image");
    metadata.insert("image".to_owned(), image_metadata(&bytes));
    CallToolResult::success(vec![
        ContentBlock::image(base64, "image/png"),
        ContentBlock::text(
            serde_json::to_string_pretty(&Value::Object(metadata))
                .unwrap_or_else(|_| "null".to_owned()),
        ),
    ])
}

async fn validate_region(args: &Map<String, Value>, context: &ToolContext) -> CallToolResult {
    let project = match project_id(args) {
        Ok(project) => project,
        Err(error) => return super::bridge_error(error),
    };
    let revision = match revision(args) {
        Ok(revision) => revision,
        Err(error) => return super::bridge_error(error),
    };
    let request = match args::normalize_validation_request(args) {
        Ok(request) => request,
        Err(error) => return super::bridge_error(error),
    };
    let params = with_revision(
        json!({ "projectId": project, "request": request }),
        revision,
    );
    match desktop_call(context, "validate_region", params, None).await {
        Ok(result) => text_result(&result),
        Err(error) => error,
    }
}

async fn capture_viewport(args: &Map<String, Value>, context: &ToolContext) -> CallToolResult {
    let project = match project_id(args) {
        Ok(project) => project,
        Err(error) => return super::bridge_error(error),
    };
    let revision = match revision(args) {
        Ok(revision) => revision,
        Err(error) => return super::bridge_error(error),
    };
    let dimension = match args::optional_string(args, "dimension", 1, 128) {
        Ok(dimension) => dimension,
        Err(error) => return super::bridge_error(error),
    };
    let mut payload = with_revision(json!({ "projectId": project }), revision);
    if let Some(dimension) = dimension {
        payload["dimension"] = json!(dimension);
    }
    let result =
        match desktop_call(context, "capture_viewport", payload, Some(RENDER_TIMEOUT)).await {
            Ok(result) => result,
            Err(error) => return error,
        };
    let Some(result_object) = result.as_object() else {
        return invalid_response("the desktop endpoint did not return a capture");
    };
    let Some(data_url) = result_object.get("dataUrl").and_then(Value::as_str) else {
        return invalid_response("the desktop endpoint did not return a PNG image");
    };
    let bytes = match png_bytes(data_url) {
        Ok(bytes) => bytes,
        Err(error) => return invalid_response(error),
    };
    let base64 = data_url
        .strip_prefix(PNG_DATA_URL_PREFIX)
        .expect("png_bytes validated the prefix");
    let metadata = json!({
        "metadata": result_object.get("metadata").cloned().unwrap_or(Value::Null),
        "verification": result_object.get("verification").cloned().unwrap_or(Value::Null),
        "capturedAtMs": result_object.get("capturedAtMs").cloned().unwrap_or(Value::Null),
        // The image itself travels in the preceding content block. Publishing its size
        // and digest lets a text-only client verify what it received.
        "image": image_metadata(&bytes),
    });
    CallToolResult::success(vec![
        ContentBlock::image(base64, "image/png"),
        ContentBlock::text(
            serde_json::to_string_pretty(&metadata).unwrap_or_else(|_| "null".to_owned()),
        ),
    ])
}

fn invalid_response(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(format!(
        "invalid_response: {}",
        message.into()
    ))])
}
