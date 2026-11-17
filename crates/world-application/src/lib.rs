//! Application facade shared by the Tauri UI and the MCP bridge.
//!
//! The facade owns structured errors and (later) project state, policy, jobs and audit.
//! It never depends on Tauri or a UI framework.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use world_domain::{TARGET_DATA_VERSION, TARGET_MINECRAFT_VERSION};

pub const APP_NAME: &str = "FuryBuilder";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Stable machine-readable error codes. New codes are additive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidArgument,
    OutOfBounds,
    NotFound,
    Conflict,
    PermissionDenied,
    RevisionMismatch,
    UnsupportedVersion,
    UnsupportedFormat,
    BackendUnavailable,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[error("{code:?}: {message}")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgument, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    pub fn with_details(mut self, details: impl Into<serde_json::Value>) -> Self {
        self.details = Some(details.into());
        self
    }
}

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

    #[test]
    fn errors_serialize_with_machine_readable_code() {
        let error = AppError::invalid_argument("bad bounds");
        let json = serde_json::to_value(&error).expect("serializable");
        assert_eq!(json["code"], "invalid_argument");
        assert_eq!(json["message"], "bad bounds");
    }
}
