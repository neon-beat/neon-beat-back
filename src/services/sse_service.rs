use std::{convert::Infallible, time::Duration};

use axum::response::sse::{Event, KeepAlive, Sse};
use futures::Stream;
use tokio::sync::{
    broadcast::{self, error::RecvError},
    watch,
};
use uuid::Uuid;

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::{
    dto::sse::{Handshake, ServerEvent, SystemStatus},
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
    mut degraded_rx: watch::Receiver<bool>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // small bounded channel between forwarder and response
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(8);

    // forwarder task: reads from broadcast and pushes into mpsc
    tokio::spawn(async move {
        loop {
            // Forward either broadcast events or degraded-mode changes to the
            // client until the channel closes or the SSE sender drops.
            tokio::select! {
                _ = tx.closed() => break,
                recv_result = receiver.recv() => {
                    if !forward_broadcast(recv_result, &tx).await {
                        break;
                    }
                }
                changed = degraded_rx.changed() => {
                    match changed {
                        Ok(_) => {
                            let degraded_flag = {
                                let guard = degraded_rx.borrow();
                                *guard
                            };

                            if !forward_system_status(degraded_flag, &tx).await {
                                break;
                            }
                        }
                        Err(_) => {
                            // sender dropped; no more updates, exit loop
                            break;
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

/// Broadcast the initial handshake payload (including token) to a connecting
/// admin SSE client.
pub fn broadcast_admin_handshake(hub: &SseHub, token: &str, degraded: bool) {
    if let Ok(event) = ServerEvent::json(
        Some("handshake".to_string()),
        &Handshake {
            stream: "admin".to_string(),
            message: "admin stream connected".to_string(),
            degraded,
            token: Some(token.to_string()),
        },
    ) {
        hub.broadcast(event);
    }
}

/// Broadcast the initial handshake payload to a connecting public SSE client.
pub fn broadcast_public_handshake(hub: &SseHub, degraded: bool) {
    if let Ok(event) = ServerEvent::json(
        Some("handshake".to_string()),
        &Handshake {
            stream: "public".to_string(),
            message: "public stream connected".to_string(),
            degraded,
            token: None,
        },
    ) {
        hub.broadcast(event);
    }
}

/// Clear any stored admin token so the next admin connection negotiates a
/// fresh credential.
async fn reset_admin_token(state: SharedState) {
    let mut guard = state.admin_token().lock().await;
    guard.take();
}

/// Forward a broadcast payload to the SSE mpsc channel, handling lag and
/// closed receivers gracefully.
async fn forward_broadcast(
    recv_result: Result<ServerEvent, RecvError>,
    tx: &mpsc::Sender<Result<Event, Infallible>>,
) -> bool {
    match recv_result {
        Ok(payload) => {
            let mut event = Event::default().data(payload.data);
            if let Some(name) = payload.event {
                event = event.event(name);
            }

            tx.send(Ok(event)).await.is_ok()
        }
        Err(RecvError::Closed) => false,
        Err(RecvError::Lagged(_)) => true,
    }
}

/// Forward a system-status payload to the SSE mpsc channel.
async fn forward_system_status(
    degraded: bool,
    tx: &mpsc::Sender<Result<Event, Infallible>>,
) -> bool {
    match ServerEvent::json(
        Some("system_status".to_string()),
        &SystemStatus { degraded },
    ) {
        Ok(payload) => {
            let mut event = Event::default().data(payload.data);
            if let Some(name) = payload.event {
                event = event.event(name);
            }

            tx.send(Ok(event)).await.is_ok()
        }
        Err(err) => {
            tracing::warn!(error = %err, "failed to serialise system status event");
            true
        }
    }
}
