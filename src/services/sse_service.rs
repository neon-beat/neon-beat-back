use std::{convert::Infallible, time::Duration};

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::Stream;
use tokio::sync::broadcast::{self, error::RecvError};
use uuid::Uuid;

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    dto::sse::{AdminHandshake, ServerEvent},
    error::ServiceError,
    state::{SharedState, SseHub},
};

/// Subscribe to the shared public SSE stream.
pub fn subscribe_public(state: &SharedState) -> broadcast::Receiver<ServerEvent> {
    state.public_sse().subscribe()
}

/// Subscribe to the admin-only SSE stream.
pub async fn subscribe_admin(
    state: &SharedState,
) -> Result<(broadcast::Receiver<ServerEvent>, String), ServiceError> {
    let token = claim_admin_token(state).await?;
    let receiver = state.admin_sse().subscribe();
    Ok((receiver, token))
}

/// Identifies the target SSE stream so we can perform stream-specific
/// bookkeeping when the connection is torn down.
#[derive(Clone)]
pub enum StreamKind {
    Public,
    /// Carries a clone of the shared application state so teardown logic can
    /// reset the admin token after the spawned task completes. Cloning
    /// `SharedState` is cheap because it is just bumping the inner `Arc`.
    Admin(SharedState),
}

/// Convert a broadcast receiver into an SSE response, forwarding events and
/// cleaning up once the client disconnects.
pub fn to_sse_stream(
    mut receiver: broadcast::Receiver<ServerEvent>,
    kind: StreamKind,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // small bounded channel between forwarder and response
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(8);

    // forwarder task: reads from broadcast and pushes into mpsc
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tx.closed() => break,
                recv_result = receiver.recv() => {
                    match recv_result {
                        Ok(payload) => {
                            let mut event = Event::default().data(payload.data);
                            if let Some(name) = payload.event {
                                event = event.event(name);
                            }

                            if tx.send(Ok(event)).await.is_err() {
                                break;
                            }
                        }
                        Err(RecvError::Closed) => break,
                        Err(RecvError::Lagged(_)) => {
                            // Skip lagged messages but keep the stream alive.
                            continue;
                        }
                    }
                }
            }
        }

        match kind {
            StreamKind::Public => tracing::info!("Public SSE stream disconnected"),
            StreamKind::Admin(state) => {
                // Own the necessary state inside the spawned task so we can
                // clean up even if the request context has already dropped.
                reset_admin_token(state).await;
                tracing::info!("Admin SSE stream disconnected")
            }
        }
    });

    // response stream reads from mpsc; when client disconnects axum drops this stream
    let stream = ReceiverStream::new(rx);
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

/// Reserve the admin token for a new stream, generating one when none exists
/// and failing if another connection already holds it.
async fn claim_admin_token(state: &SharedState) -> Result<String, ServiceError> {
    let mut guard = state.admin_token().lock().await;
    match &mut *guard {
        slot @ None => {
            let token = Uuid::new_v4().simple().to_string();
            slot.replace(token.clone());
            Ok(token)
        }
        Some(_) => Err(ServiceError::Unauthorized(
            "Another admin SSE stream is already active".into(),
        )),
    }
}

/// Broadcast a token refresh event to the admin stream.
pub fn broadcast_admin_handshake(hub: &SseHub, token: &str) {
    if let Ok(event) = ServerEvent::json(
        Some("admin_token".to_string()),
        &AdminHandshake {
            token: token.to_string(),
        },
    ) {
        hub.broadcast(event);
    }
}

/// Send a human-readable info message onto the public SSE stream.
pub fn broadcast_public_info(hub: &SseHub, message: &str) {
    hub.broadcast(ServerEvent::new(
        Some("info".to_string()),
        message.to_string(),
    ));
}

/// Clear any stored admin token so the next admin connection negotiates a
/// fresh credential.
async fn reset_admin_token(state: SharedState) {
    let mut guard = state.admin_token().lock().await;
    guard.take();
}
