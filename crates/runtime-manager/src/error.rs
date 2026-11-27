use thiserror::Error;

use download_pipeline::PipelineError;

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
    #[error("network access is disabled (offline mode)")]
    Offline,
    #[error("runtime download failed an integrity check: {0}")]
    Checksum(String),
    #[error("not enough free disk space: {0}")]
    DiskSpace(String),
    #[error("runtime archive is not safe or not a Temurin runtime: {0}")]
    Archive(String),
    #[error("runtime limit exceeded: {0}")]
    Limit(String),
    #[error("runtime cache is busy: {0}")]
    Busy(String),
    #[error("runtime install was cancelled")]
    Cancelled,
}

impl RuntimeError {
    pub fn code(&self) -> &'static str {
        match self {
            RuntimeError::Io(_) => "io_error",
            RuntimeError::Invalid(_) => "invalid_argument",
            RuntimeError::Unsupported { .. } => "unsupported_version",
            RuntimeError::Network(_) => "network_error",
            RuntimeError::Offline => "offline",
            RuntimeError::Checksum(_) => "checksum_mismatch",
            RuntimeError::DiskSpace(_) => "disk_space",
            RuntimeError::Archive(_) => "archive_error",
            RuntimeError::Limit(_) => "limit_exceeded",
            RuntimeError::Busy(_) => "busy",
            RuntimeError::Cancelled => "cancelled",
        }
    }
}

impl From<PipelineError> for RuntimeError {
    fn from(error: PipelineError) -> Self {
        let message = error.to_string();
        match error {
            PipelineError::Offline => RuntimeError::Offline,
            PipelineError::Cancelled => RuntimeError::Cancelled,
            PipelineError::Checksum { .. } => RuntimeError::Checksum(message),
            PipelineError::Size { .. } | PipelineError::Limit(_) => RuntimeError::Limit(message),
            PipelineError::InsufficientSpace { .. } => RuntimeError::DiskSpace(message),
            PipelineError::Io(_) => RuntimeError::Io(message),
            PipelineError::DisallowedUrl(_)
            | PipelineError::Proxy(_)
            | PipelineError::Http { .. }
            | PipelineError::Transport(_) => RuntimeError::Network(message),
        }
    }
}

pub fn unsupported(detail: impl Into<String>) -> RuntimeError {
    RuntimeError::Unsupported {
        target: crate::target::target_minecraft_version(),
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
    Unsupported { target: String, detail: String },
    #[error("world generation is not available for {profile_id}: {detail}")]
    UnsupportedGeneration { profile_id: String, detail: String },
    #[error("world conversion is not available for {profile_id}: {detail}")]
    UnsupportedConversion { profile_id: String, detail: String },
    #[error("server JAR network request failed: {0}")]
    Network(String),
    #[error("network access is disabled (offline mode)")]
    Offline,
    #[error("server JAR failed an integrity check: {0}")]
    Checksum(String),
    #[error("not enough free disk space: {0}")]
    DiskSpace(String),
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
    UnsupportedJava { target: String, detail: String },
    #[error("server JAR install was cancelled")]
    Cancelled,
    #[error("world conversion verification failed: {0}")]
    Verification(String),
}

impl ServerError {
    pub fn code(&self) -> &'static str {
        match self {
            ServerError::Io(_) => "io_error",
            ServerError::Invalid(_) => "invalid_argument",
            ServerError::Unsupported { .. } => "unsupported_version",
            ServerError::UnsupportedGeneration { .. } => "unsupported_version",
            ServerError::UnsupportedConversion { .. } => "unsupported_version",
            ServerError::Network(_) => "network_error",
            ServerError::Offline => "offline",
            ServerError::Checksum(_) => "checksum_mismatch",
            ServerError::DiskSpace(_) => "disk_space",
            ServerError::Archive(_) => "archive_error",
            ServerError::Limit(_) => "limit_exceeded",
            ServerError::Busy(_) => "busy",
            ServerError::EulaRequired(_) => "eula_required",
            ServerError::ConsentRequired(_) => "consent_required",
            ServerError::UnsupportedJava { .. } => "unsupported_runtime",
            ServerError::Cancelled => "cancelled",
            ServerError::Verification(_) => "verification_failed",
        }
    }
}

impl From<PipelineError> for ServerError {
    fn from(error: PipelineError) -> Self {
        let message = error.to_string();
        match error {
            PipelineError::Offline => ServerError::Offline,
            PipelineError::Cancelled => ServerError::Cancelled,
            PipelineError::Checksum { .. } => ServerError::Checksum(message),
            PipelineError::Size { .. } | PipelineError::Limit(_) => ServerError::Limit(message),
            PipelineError::InsufficientSpace { .. } => ServerError::DiskSpace(message),
            PipelineError::Io(_) => ServerError::Io(message),
            PipelineError::DisallowedUrl(_)
            | PipelineError::Proxy(_)
            | PipelineError::Http { .. }
            | PipelineError::Transport(_) => ServerError::Network(message),
        }
    }
}
