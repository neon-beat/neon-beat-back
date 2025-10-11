use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use thiserror::Error;

use crate::{
    dao::storage::StorageError,
    state::{AbortError, ApplyError, PlanError},
};

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("storage unavailable")]
    Unavailable(#[source] StorageError),
    #[error("storage unavailable (degraded mode)")]
    Degraded,
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("invalid state: {0}")]
    InvalidState(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("operation timed out")]
    Timeout,
}

impl From<StorageError> for ServiceError {
    fn from(err: StorageError) -> Self {
        ServiceError::Unavailable(err)
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<ServiceError> for AppError {
    fn from(err: ServiceError) -> Self {
        match err {
            ServiceError::Unavailable(source) => AppError::ServiceUnavailable(source.to_string()),
            ServiceError::Degraded => AppError::ServiceUnavailable("degraded mode".into()),
            ServiceError::Unauthorized(message) => AppError::Unauthorized(message),
            ServiceError::InvalidInput(message) => AppError::BadRequest(message),
            ServiceError::InvalidState(message) => AppError::Conflict(message),
            ServiceError::NotFound(message) => AppError::NotFound(message),
            ServiceError::Timeout => AppError::ServiceUnavailable("operation timed out".into()),
        }
    }
}

#[derive(Serialize)]
struct ErrorBody {
    message: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let status = match &self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let payload = Json(ErrorBody {
            message: self.to_string(),
        });

        (status, payload).into_response()
    }
}

impl From<PlanError> for ServiceError {
    fn from(err: PlanError) -> Self {
        match err {
            PlanError::AlreadyPending => {
                ServiceError::InvalidState("state transition already pending".into())
            }
            PlanError::InvalidTransition(invalid) => {
                ServiceError::InvalidState(invalid.to_string())
            }
        }
    }
}

impl From<ApplyError> for ServiceError {
    fn from(err: ApplyError) -> Self {
        match err {
            ApplyError::NoPending => ServiceError::InvalidState("no transition is pending".into()),
            ApplyError::IdMismatch { .. } => {
                ServiceError::InvalidState("pending transition does not match".into())
            }
            ApplyError::PhaseMismatch { expected, actual } => ServiceError::InvalidState(format!(
                "state changed during transition (expected {expected:?}, got {actual:?})"
            )),
            ApplyError::VersionMismatch { expected, actual } => {
                ServiceError::InvalidState(format!(
                    "state version mismatch during transition (expected {expected}, got {actual})"
                ))
            }
        }
    }
}

impl From<AbortError> for ServiceError {
    fn from(err: AbortError) -> Self {
        match err {
            AbortError::NoPending => ServiceError::InvalidState("no pending transition".into()),
            AbortError::IdMismatch { .. } => {
                ServiceError::InvalidState("transition plan does not match".into())
            }
        }
    }
}
