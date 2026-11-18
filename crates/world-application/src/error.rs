use serde::{Deserialize, Serialize};
use thiserror::Error;
use world_domain::{BoundsError, RegistryError, WorldError};
use world_operations::OperationError;

/// Stable machine-readable error codes. New codes are additive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidArgument,
    OutOfBounds,
    NotFound,
    Conflict,
    PermissionDenied,
    Unauthorized,
    RevisionMismatch,
    LimitExceeded,
    UnsupportedVersion,
    UnsupportedFormat,
    BackendUnavailable,
    NoHistory,
    Cancelled,
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

impl From<WorldError> for AppError {
    fn from(error: WorldError) -> Self {
        match &error {
            WorldError::UnknownDimension(_) => {
                AppError::new(ErrorCode::NotFound, error.to_string())
            }
            WorldError::Dimension(_) => AppError::new(ErrorCode::OutOfBounds, error.to_string()),
        }
    }
}

impl From<BoundsError> for AppError {
    fn from(error: BoundsError) -> Self {
        match error {
            BoundsError::TooLarge { .. } => {
                AppError::new(ErrorCode::LimitExceeded, error.to_string())
            }
            BoundsError::Inverted => AppError::invalid_argument(error.to_string()),
        }
    }
}

impl From<RegistryError> for AppError {
    fn from(error: RegistryError) -> Self {
        AppError::invalid_argument(error.to_string())
    }
}

impl From<OperationError> for AppError {
    fn from(error: OperationError) -> Self {
        match &error {
            OperationError::UnknownDimension(_)
            | OperationError::World(WorldError::UnknownDimension(_)) => {
                AppError::new(ErrorCode::NotFound, error.to_string())
            }
            OperationError::TooManyBlocks { .. } => {
                AppError::new(ErrorCode::LimitExceeded, error.to_string())
            }
            OperationError::OutOfBounds { .. }
            | OperationError::World(WorldError::Dimension(_)) => {
                AppError::new(ErrorCode::OutOfBounds, error.to_string())
            }
            OperationError::Bounds(BoundsError::TooLarge { .. }) => {
                AppError::new(ErrorCode::LimitExceeded, error.to_string())
            }
            OperationError::Bounds(_) | OperationError::EmptyBatch => {
                AppError::invalid_argument(error.to_string())
            }
            OperationError::InvalidBrush(_) => AppError::invalid_argument(error.to_string()),
            OperationError::Cancelled => AppError::new(ErrorCode::Cancelled, error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_serialize_with_machine_readable_code() {
        let error = AppError::invalid_argument("bad bounds");
        let json = serde_json::to_value(&error).expect("serializable");
        assert_eq!(json["code"], "invalid_argument");
        assert_eq!(json["message"], "bad bounds");
    }
}
