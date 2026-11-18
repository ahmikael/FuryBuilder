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

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("server JAR files cannot be read or written: {0}")]
    Io(String),
    #[error("server JAR request is not valid: {0}")]
    Invalid(String),
    #[error("server JAR does not match Minecraft {target}: {detail}")]
    Unsupported {
        target: &'static str,
        detail: String,
    },
    #[error("server JAR network request failed: {0}")]
    Network(String),
    #[error("server JAR failed an integrity check: {0}")]
    Checksum(String),
    #[error("server JAR is not a safe or supported Minecraft server archive: {0}")]
    Archive(String),
    #[error("server JAR limit exceeded: {0}")]
    Limit(String),
    #[error("server JAR cache is busy: {0}")]
    Busy(String),
    #[error("Minecraft EULA confirmation is required: {0}")]
    EulaRequired(String),
    #[error("explicit generation consent is required: {0}")]
    ConsentRequired(String),
    #[error("Java runtime is not compatible with Minecraft {target}: {detail}")]
    UnsupportedJava {
        target: &'static str,
        detail: String,
    },
}

impl ServerError {
    pub fn code(&self) -> &'static str {
        match self {
            ServerError::Io(_) => "io_error",
            ServerError::Invalid(_) => "invalid_argument",
            ServerError::Unsupported { .. } => "unsupported_version",
            ServerError::Network(_) => "network_error",
            ServerError::Checksum(_) => "checksum_mismatch",
            ServerError::Archive(_) => "archive_error",
            ServerError::Limit(_) => "limit_exceeded",
            ServerError::Busy(_) => "busy",
            ServerError::EulaRequired(_) => "eula_required",
            ServerError::ConsentRequired(_) => "consent_required",
            ServerError::UnsupportedJava { .. } => "unsupported_runtime",
        }
    }
}

pub fn unsupported_server(detail: impl Into<String>) -> ServerError {
    ServerError::Unsupported {
        target: crate::types::TARGET_MINECRAFT_VERSION,
        detail: detail.into(),
    }
}
