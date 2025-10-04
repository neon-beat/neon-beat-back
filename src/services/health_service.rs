use tracing::warn;

use crate::{dto::health::HealthResponse, state::SharedState};

/// Respond with a static health payload while logging connectivity issues.
pub async fn health_status(state: &SharedState) -> HealthResponse {
    match state.mongo().await {
        Some(mongo) => {
            if let Err(err) = mongo.ping().await {
                warn!(error = %err, "mongodb ping failed");
            }
        }
        None => warn!("mongodb unavailable (degraded mode)"),
    }

    if state.is_degraded() {
        HealthResponse::degraded()
    } else {
        HealthResponse::ok()
    }
}
