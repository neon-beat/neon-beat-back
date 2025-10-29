use serde::Serialize;
use utoipa::ToSchema;

/// Simple health response returned by the `/healthcheck` route.
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    /// Health status ("ok" or "degraded").
    pub status: String,
}

impl HealthResponse {
    /// Create a health response indicating the system is operational.
    pub fn ok() -> Self {
        Self {
            status: "ok".to_string(),
        }
    }

    /// Create a health response indicating the system is in degraded mode.
    pub fn degraded() -> Self {
        Self {
            status: "degraded".to_string(),
        }
    }
}
