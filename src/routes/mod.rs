use axum::{
    Router,
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde_json::json;

use crate::state::SharedState;

pub mod admin;
pub mod docs;
pub mod health;
pub mod public;
pub mod sse;
pub mod websocket;

/// Compose all route trees, wiring in shared state and documentation routes.
pub fn router(state: SharedState) -> Router<()> {
    let api_router = health::router()
        .merge(sse::router())
        .merge(websocket::router())
        .merge(public::router())
        .merge(admin::router(state.clone()));

    let docs_router = docs::router(state.clone());

    api_router
        .merge(docs_router)
        .fallback(fallback_handler)
        .with_state(state)
}

/// Fallback handler for routes that don't match any defined endpoints.
/// Returns a 404 Not Found with a JSON error message.
async fn fallback_handler() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "Not Found",
            "message": "The requested resource does not exist"
        })),
    )
}
