//! Capability-independent job tools.
//!
//! They are the required fallback when a client does not negotiate the optional Tasks
//! extension, and they remain authoritative even when Tasks becomes available. The bridge
//! only transports requests; queueing, execution, progress and cancellation live in the
//! Rust Core.

use rmcp::model::CallToolResult;
use serde_json::{Map, Value};

use crate::args;
use crate::desktop::{self, StartJobRequest};

use super::{job_error, project_id, text_result, ToolContext};

pub async fn start_job(args: &Map<String, Value>, context: &ToolContext) -> CallToolResult {
    let project_id = match project_id(args) {
        Ok(project_id) => project_id,
        Err(error) => return super::bridge_error(error),
    };
    let kind = match args::required_string(args, "kind", 1, 64) {
        Ok(kind) => kind,
        Err(error) => return super::bridge_error(error),
    };
    let parameters = match args.get("parameters") {
        None | Some(Value::Null) => Map::new(),
        Some(value) => match value.as_object() {
            Some(parameters) => parameters.clone(),
            None => return super::bridge_error("parameters must be an object"),
        },
    };
    let idempotency_key = match args::required_string(args, "idempotency_key", 1, 128) {
        Ok(key) => key,
        Err(error) => return super::bridge_error(error),
    };
    let expected_revision = match args::optional_revision(args, "world_revision") {
        Ok(Some(revision)) => Some(revision),
        Ok(None) => match args::optional_revision(args, "expected_revision") {
            Ok(revision) => revision,
            Err(error) => return super::bridge_error(error),
        },
        Err(error) => return super::bridge_error(error),
    };

    let request = StartJobRequest {
        project_id,
        kind,
        parameters,
        idempotency_key,
        expected_revision,
    };
    match desktop::start_job(&request, &context.options()).await {
        Ok(snapshot) => text_result(&snapshot),
        Err(error) => job_error(error),
    }
}

async fn query(args: &Map<String, Value>, context: &ToolContext, cancel: bool) -> CallToolResult {
    let project_id = match project_id(args) {
        Ok(project_id) => project_id,
        Err(error) => return super::bridge_error(error),
    };
    let job_id = match args::required_string(args, "job_id", 1, 128) {
        Ok(job_id) => job_id,
        Err(error) => return super::bridge_error(error),
    };
    let result = if cancel {
        desktop::cancel_job(&project_id, &job_id, &context.options()).await
    } else {
        desktop::job_status(&project_id, &job_id, &context.options()).await
    };
    match result {
        Ok(snapshot) => text_result(&snapshot),
        Err(error) => job_error(error),
    }
}

pub async fn get_job_status(args: &Map<String, Value>, context: &ToolContext) -> CallToolResult {
    query(args, context, false).await
}

pub async fn cancel_job(args: &Map<String, Value>, context: &ToolContext) -> CallToolResult {
    query(args, context, true).await
}
