use axum::Router;

use crate::state::SharedState;

pub mod docs;
pub mod game;
pub mod health;
pub mod sse;
pub mod websocket;

/// Compose all route trees, wiring in shared state and documentation routes.
pub fn router(state: SharedState) -> Router<()> {
    let api_router = health::router()
        .merge(sse::router())
        .merge(websocket::router())
        .merge(game::router());

    let docs_router = docs::router(state.clone());

    api_router.merge(docs_router).with_state(state)
}
