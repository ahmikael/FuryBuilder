//! Application facade shared by the Tauri UI and the MCP bridge.
//!
//! The facade owns structured errors, the authoritative world service, revisions,
//! idempotency and the audit trail. It never depends on Tauri or a UI framework.

mod dto;
mod error;
mod jobs;
mod service;

pub use dto::{
    Actor, ActorSource, AuditAction, AuditEntry, ChunkSnapshot, ChunkSnapshotBatch, ChunkStats,
    HistoryState, HistorySummary, OperationRequest, OperationResult, PaletteEntry, PreviewResult,
    Query, QueryResult, RegionBlocks, RegionSummary, RenderSnapshot, SectionSnapshot,
    StructureSites, UndoTarget,
};
pub use error::{AppError, ErrorCode};
pub use jobs::{
    apply_job_operation, undo_job_operation_id, JobQueue, JobSnapshot, JobStartRequest, JobState,
    DEFAULT_JOB_LIST_LIMIT, MAX_JOB_LIST_LIMIT, MAX_QUEUED_JOBS, MAX_RETAINED_JOBS,
};
pub use service::{
    WorldService, DEFAULT_MAX_HISTORY, MAX_IDEMPOTENCY_ENTRIES, MAX_REGION_READ_BLOCKS,
    MAX_REGION_SUMMARY_BLOCKS, MAX_RENDER_CHUNKS, MAX_RENDER_RADIUS,
};
pub use workspace_model::{
    Layer, WorkspaceError, WorkspacePolicy, WorkspaceRegion, MAX_LAYERS, MAX_MASKS,
    MAX_NAME_CHARS as MAX_WORKSPACE_NAME_CHARS, MAX_PROTECTED_REGIONS, MAX_REGION_VOLUME,
    WORKSPACE_SCHEMA_VERSION,
};
pub use world_operations::{BlockChange, Operation, Selection, TerrainBrushKind};

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use world_domain::{TARGET_DATA_VERSION, TARGET_MINECRAFT_VERSION};

pub const APP_NAME: &str = "FuryBuilder";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub target_minecraft_version: String,
    pub target_data_version: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PingResponse {
    pub message: String,
    pub app_version: String,
    pub timestamp_ms: u128,
}

pub fn app_info() -> AppInfo {
    AppInfo {
        name: APP_NAME.to_owned(),
        version: APP_VERSION.to_owned(),
        target_minecraft_version: TARGET_MINECRAFT_VERSION.to_owned(),
        target_data_version: TARGET_DATA_VERSION,
    }
}

/// Returns a typed probe response used by the UI and MCP setup check.
pub fn ping(name: Option<&str>) -> Result<PingResponse, AppError> {
    let name = name.unwrap_or("world").trim();
    if name.len() > 64 {
        return Err(AppError::invalid_argument(
            "name must be at most 64 characters",
        ));
    }
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| AppError::internal(format!("system clock error: {error}")))?
        .as_millis();
    Ok(PingResponse {
        message: format!("pong, {name}"),
        app_version: APP_VERSION.to_owned(),
        timestamp_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_matches_target_version() {
        let info = app_info();
        assert_eq!(info.name, "FuryBuilder");
        assert_eq!(info.target_minecraft_version, "1.21.4");
        assert_eq!(info.target_data_version, 4189);
    }

    #[test]
    fn ping_returns_typed_response() {
        let response = ping(Some("agent")).expect("valid ping");
        assert_eq!(response.message, "pong, agent");
        assert!(response.timestamp_ms > 0);
    }

    #[test]
    fn ping_rejects_overlong_names() {
        let long = "a".repeat(65);
        let error = ping(Some(&long)).expect_err("must reject");
        assert_eq!(error.code, ErrorCode::InvalidArgument);
    }
}
