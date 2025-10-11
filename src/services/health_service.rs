use tracing::warn;

use crate::{dto::health::HealthResponse, state::SharedState};

/// Respond with a static health payload while logging connectivity issues.
pub async fn health_status(state: &SharedState) -> HealthResponse {
    match state.game_store().await {
        Some(store) => {
            if let Err(err) = store.health_check().await {
                warn!(error = %err, "storage health check failed");
            }
        }
        None => warn!("storage unavailable (degraded mode)"),
    }

    if state.is_degraded().await {
        HealthResponse::degraded()
    } else {
        HealthResponse::ok()
    }
}
