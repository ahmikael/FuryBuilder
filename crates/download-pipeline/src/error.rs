//! One error vocabulary for every managed download.

use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("network access is disabled (offline mode)")]
    Offline,
    #[error("refusing a download from an untrusted or malformed URL: {0}")]
    DisallowedUrl(String),
    #[error("invalid proxy configuration: {0}")]
    Proxy(String),
    #[error("HTTP {status} from {url}")]
    Http {
        status: u16,
        url: String,
        retry_after: Option<Duration>,
    },
    #[error("network error: {0}")]
    Transport(String),
    #[error("download I/O error: {0}")]
    Io(String),
    #[error("download exceeds the allowed limit: {0}")]
    Limit(String),
    #[error("download size mismatch: expected {expected} bytes, received {actual}")]
    Size { expected: u64, actual: u64 },
    #[error("download checksum mismatch: expected {expected}, received {actual}")]
    Checksum { expected: String, actual: String },
    #[error(
        "not enough free disk space at {path}: need {required} bytes, {available} bytes available"
    )]
    InsufficientSpace {
        path: String,
        required: u64,
        available: u64,
    },
    #[error("download was cancelled")]
    Cancelled,
}

impl PipelineError {
    /// Only transport failures and transient server statuses are worth retrying. A checksum,
    /// size, policy or disk-space failure is definitive and must reach the user unchanged.
    pub fn is_retryable(&self) -> bool {
        match self {
            PipelineError::Http { status, .. } => {
                matches!(status, 408 | 425 | 429 | 500 | 502 | 503 | 504)
            }
            PipelineError::Transport(_) => true,
            _ => false,
        }
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            PipelineError::Http { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            PipelineError::Offline => "offline",
            PipelineError::DisallowedUrl(_) | PipelineError::Proxy(_) => "invalid_argument",
            PipelineError::Http { .. } | PipelineError::Transport(_) => "network_error",
            PipelineError::Io(_) => "io_error",
            PipelineError::Limit(_) | PipelineError::Size { .. } => "limit_exceeded",
            PipelineError::Checksum { .. } => "checksum_mismatch",
            PipelineError::InsufficientSpace { .. } => "disk_space",
            PipelineError::Cancelled => "cancelled",
        }
    }
}

impl From<std::io::Error> for PipelineError {
    fn from(error: std::io::Error) -> Self {
        PipelineError::Io(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_transient_failures_are_retried() {
        for status in [408, 425, 429, 500, 502, 503, 504] {
            assert!(PipelineError::Http {
                status,
                url: "https://piston-data.mojang.com/x".to_owned(),
                retry_after: None,
            }
            .is_retryable());
        }
        for status in [400, 401, 403, 404, 410] {
            assert!(!PipelineError::Http {
                status,
                url: "https://piston-data.mojang.com/x".to_owned(),
                retry_after: None,
            }
            .is_retryable());
        }
        assert!(PipelineError::Transport("reset".to_owned()).is_retryable());
        assert!(!PipelineError::Offline.is_retryable());
        assert!(!PipelineError::Checksum {
            expected: "a".repeat(40),
            actual: "b".repeat(40),
        }
        .is_retryable());
        assert!(!PipelineError::InsufficientSpace {
            path: "C:\\cache".to_owned(),
            required: 10,
            available: 1,
        }
        .is_retryable());
    }
}
