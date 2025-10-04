use std::convert::Infallible;

use axum::{Router, extract::State, response::sse::Sse, routing::get};
use futures::Stream;
use tracing::info;

use crate::{
    error::AppError,
    services::sse_service::{self, StreamKind},
    state::SharedState,
};

#[utoipa::path(
    get,
    path = "/sse/public",
    responses((status = 200, description = "Public SSE stream", content_type = "text/event-stream", body = String))
)]
/// Stream realtime public events to connected frontends.
pub async fn public_stream(
    State(state): State<SharedState>,
) -> Sse<impl Stream<Item = Result<axum::response::sse::Event, Infallible>>> {
    let receiver = sse_service::subscribe_public(&state);
    info!("New public SSE connection");
    sse_service::broadcast_public_info(state.public_sse(), "public stream connected");
    sse_service::to_sse_stream(receiver, StreamKind::Public)
}

#[utoipa::path(
    get,
    path = "/sse/admin",
    responses((status = 200, description = "Admin SSE stream", content_type = "text/event-stream", body = String))
)]
/// Stream admin-only events, establishing or validating the admin token.
pub async fn admin_stream(
    State(state): State<SharedState>,
) -> Result<Sse<impl Stream<Item = Result<axum::response::sse::Event, Infallible>>>, AppError> {
    let (receiver, token) = sse_service::subscribe_admin(&state).await?;
    info!("New admin SSE connection");
    sse_service::broadcast_admin_handshake(state.admin_sse(), &token);
    Ok(sse_service::to_sse_stream(
        receiver,
        StreamKind::Admin(state),
    ))
}

/// Configure the SSE endpoints.
pub fn router() -> Router<SharedState> {
    Router::<SharedState>::new()
        .route("/sse/public", get(public_stream))
        .route("/sse/admin", get(admin_stream))
}
