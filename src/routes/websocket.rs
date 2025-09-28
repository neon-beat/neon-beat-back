use axum::{
    Router,
    extract::{State, WebSocketUpgrade},
    response::IntoResponse,
    routing::get,
};

use crate::{services::websocket_service, state::SharedState};

#[utoipa::path(
    get,
    path = "/ws",
    responses((status = 101, description = "Switching protocols to WebSocket"))
)]
/// Upgrade the HTTP connection into a buzzer WebSocket session.
pub async fn ws_handler(
    State(state): State<SharedState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let shared_state = state.clone();
    ws.on_upgrade(move |socket| websocket_service::handle_socket(shared_state.clone(), socket))
}

/// Configure the WebSocket endpoint.
pub fn router() -> Router<SharedState> {
    Router::<SharedState>::new().route("/ws", get(ws_handler))
}
