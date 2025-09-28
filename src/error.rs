use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use thiserror::Error;

use crate::dao::mongodb::MongoDaoError;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("storage unavailable")]
    Unavailable(#[source] MongoDaoError),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
}

impl From<MongoDaoError> for ServiceError {
    fn from(err: MongoDaoError) -> Self {
        match err {
            MongoDaoError::EnsureIndex { .. }
            | MongoDaoError::ClientConstruction { .. }
            | MongoDaoError::InvalidUri { .. }
            | MongoDaoError::InitialPing { .. }
            | MongoDaoError::HealthPing { .. } => ServiceError::Unavailable(err),
        }
    }
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<ServiceError> for AppError {
    fn from(err: ServiceError) -> Self {
        match err {
            ServiceError::Unavailable(source) => AppError::ServiceUnavailable(source.to_string()),
            ServiceError::Unauthorized(message) => AppError::Unauthorized(message),
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
            AppError::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        let payload = Json(ErrorBody {
            message: self.to_string(),
        });

        (status, payload).into_response()
    }
}
