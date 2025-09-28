use axum::{Json, Router, extract::State, routing::get};

use crate::{dto::health::HealthResponse, services::health_service, state::SharedState};

#[utoipa::path(
    get,
    path = "/healthcheck",
    responses((status = 200, description = "Service is healthy", body = HealthResponse))
)]
/// Return the current health status of the backend and ping MongoDB.
pub async fn healthcheck(State(state): State<SharedState>) -> Json<HealthResponse> {
    let status = health_service::health_status(&state).await;
    Json(status)
}

/// Configure the health routes subtree.
pub fn router() -> Router<SharedState> {
    Router::<SharedState>::new().route("/healthcheck", get(healthcheck))
}
