use tracing::warn;

use crate::{dto::health::HealthResponse, state::SharedState};

/// Respond with a static health payload while logging connectivity issues.
pub async fn health_status(state: &SharedState) -> HealthResponse {
    if let Err(err) = state.mongo().ping().await {
        warn!(error = %err, "mongodb ping failed");
    }

    HealthResponse::ok()
}
