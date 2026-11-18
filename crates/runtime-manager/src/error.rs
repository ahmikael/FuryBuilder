use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("runtime files cannot be read or written: {0}")]
    Io(String),
    #[error("runtime request is not valid: {0}")]
    Invalid(String),
    #[error("runtime is not compatible with Minecraft {target}: {detail}")]
    Unsupported {
        target: &'static str,
        detail: String,
    },
    #[error("runtime network request failed: {0}")]
    Network(String),
    #[error("runtime download failed an integrity check: {0}")]
    Checksum(String),
    #[error("runtime archive is not safe or not a Temurin JRE: {0}")]
    Archive(String),
    #[error("runtime limit exceeded: {0}")]
    Limit(String),
    #[error("runtime cache is busy: {0}")]
    Busy(String),
}

impl RuntimeError {
    pub fn code(&self) -> &'static str {
        match self {
            RuntimeError::Io(_) => "io_error",
            RuntimeError::Invalid(_) => "invalid_argument",
            RuntimeError::Unsupported { .. } => "unsupported_version",
            RuntimeError::Network(_) => "network_error",
            RuntimeError::Checksum(_) => "checksum_mismatch",
            RuntimeError::Archive(_) => "archive_error",
            RuntimeError::Limit(_) => "limit_exceeded",
            RuntimeError::Busy(_) => "busy",
        }
    }
}

pub fn unsupported(detail: impl Into<String>) -> RuntimeError {
    RuntimeError::Unsupported {
        target: crate::types::TARGET_MINECRAFT_VERSION,
        detail: detail.into(),
    }
}
