use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use thiserror::Error;

use crate::dao::mongodb::MongoDaoError;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("storage unavailable")]
    Unavailable(#[source] MongoDaoError),
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
}

impl From<MongoDaoError> for ServiceError {
    fn from(err: MongoDaoError) -> Self {
        match err {
            MongoDaoError::EnsureIndex { .. }
            | MongoDaoError::ClientConstruction { .. }
            | MongoDaoError::InvalidUri { .. }
            | MongoDaoError::InitialPing { .. }
            | MongoDaoError::HealthPing { .. }
            | MongoDaoError::SaveGame { .. }
            | MongoDaoError::SavePlaylist { .. }
            | MongoDaoError::LoadGame { .. }
            | MongoDaoError::LoadPlaylist { .. } => ServiceError::Unavailable(err),
        }
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
