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
    #[error("project schema version {found} is newer than supported version {supported}; open it read-only or use a newer FuryBuilder")]
    UnsupportedSchema { found: u32, supported: u32 },
    #[error("project data is corrupt: {0}")]
    Corrupt(String),
    #[error("project snapshot checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("project limit exceeded: {0}")]
    LimitExceeded(String),
    #[error("unsafe source path: {0}")]
    UnsafeSource(String),
    #[error("project manifest is invalid: {0}")]
    InvalidManifest(String),
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
            ProjectError::UnsupportedSchema { .. } => "unsupported_version",
            ProjectError::Corrupt(_) | ProjectError::ChecksumMismatch { .. } => "conflict",
            ProjectError::LimitExceeded(_) => "limit_exceeded",
            ProjectError::UnsafeSource(_) => "permission_denied",
        }
    }
}
