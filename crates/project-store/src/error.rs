use std::io;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProjectError {
    #[error("project storage I/O failed: {context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },
    #[error("project manifest is invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("project `{0}` was not found")]
    NotFound(String),
    #[error("project `{0}` already exists")]
    AlreadyExists(String),
    #[error("invalid project name: {0}")]
    InvalidName(String),
    #[error("invalid project path: {0}")]
    InvalidPath(String),
    #[error(
        "stored schema version {found} is not supported; this build reads version {supported} only"
    )]
    UnsupportedSchema { found: u32, supported: u32 },
    #[error("world version is not supported by this build: {0}")]
    UnsupportedWorldVersion(String),
    #[error("invalid flat source: {0}")]
    InvalidFlatPreset(String),
    #[error("project data is corrupt: {0}")]
    Corrupt(String),
    #[error("project snapshot checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("project limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("invalid checkpoint: {0}")]
    InvalidCheckpoint(String),
    #[error("unsafe source path: {0}")]
    UnsafeSource(String),
    #[error("project manifest is invalid: {0}")]
    InvalidManifest(String),
    #[error("version upgrade is not available: {0}")]
    UpgradeUnsupported(String),
    #[error("version upgrade is blocked: {0}")]
    UpgradeBlocked(String),
}

impl ProjectError {
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    pub fn corrupt(message: impl Into<String>) -> Self {
        Self::Corrupt(message.into())
    }

    pub fn invalid_manifest(message: impl Into<String>) -> Self {
        Self::InvalidManifest(message.into())
    }

    /// Stable machine-readable code used by the IPC layer.
    pub fn code(&self) -> &'static str {
        match self {
            ProjectError::Io { .. } => "internal",
            ProjectError::Json(_) | ProjectError::InvalidManifest(_) => "unsupported_format",
            ProjectError::NotFound(_) => "not_found",
            ProjectError::AlreadyExists(_) => "conflict",
            ProjectError::InvalidName(_) | ProjectError::InvalidPath(_) => "invalid_argument",
            ProjectError::InvalidCheckpoint(_) => "invalid_argument",
            ProjectError::UnsupportedSchema { .. } | ProjectError::UnsupportedWorldVersion(_) => {
                "unsupported_version"
            }
            ProjectError::UpgradeUnsupported(_) => "unsupported_version",
            ProjectError::UpgradeBlocked(_) => "conflict",
            ProjectError::InvalidFlatPreset(_) => "invalid_argument",
            ProjectError::Corrupt(_) | ProjectError::ChecksumMismatch { .. } => "conflict",
            ProjectError::LimitExceeded(_) => "limit_exceeded",
            ProjectError::UnsafeSource(_) => "permission_denied",
        }
    }
}
