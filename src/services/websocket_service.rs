use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{info, warn};

use crate::{
    dto::ws::{BuzzFeedback, BuzzerAck, BuzzerInboundMessage},
    error::ServiceError,
    state::{
        BuzzerConnection, SharedState,
        state_machine::{GameEvent, PauseKind},
        transitions::run_transition_with_broadcast,
    },
};

const IDENT_TIMEOUT: Duration = Duration::from_secs(10);

/// Handle the full lifecycle for an individual buzzer WebSocket connection.
pub async fn handle_socket(state: SharedState, socket: WebSocket) {
    let (mut sender, mut receiver) = socket.split();
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<Message>();

    // Dedicated writer task keeps outbound messages flowing even while we await inbound frames.
    let writer_task = tokio::spawn(async move {
        while let Some(message) = outbound_rx.recv().await {
            if sender.send(message).await.is_err() {
                break;
            }
        }
    });

    let initial_message = match tokio::time::timeout(IDENT_TIMEOUT, receiver.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => text,
        Ok(Some(Ok(Message::Close(_)))) => {
            finalize(writer_task, outbound_tx).await;
            return;
        }
        Ok(Some(Ok(_))) => {
            let _ = outbound_tx.send(Message::Close(None));
            finalize(writer_task, outbound_tx).await;
            return;
        }
        Ok(Some(Err(err))) => {
            warn!(error = %err, "websocket receive error");
            finalize(writer_task, outbound_tx).await;
            return;
        }
        Ok(None) | Err(_) => {
            warn!("websocket identification timed out");
            finalize(writer_task, outbound_tx).await;
            return;
        }
    };

    let inbound: BuzzerInboundMessage = match serde_json::from_str(&initial_message) {
        Ok(message) => message,
        Err(err) => {
            warn!(error = %err, "invalid json from buzzer");
            let _ = outbound_tx.send(Message::Close(None));
            finalize(writer_task, outbound_tx).await;
            return;
        }
    };

    let Some(buzzer_id) = inbound.identification_id() else {
        warn!("first message was not identification");
        let _ = outbound_tx.send(Message::Close(None));
        finalize(writer_task, outbound_tx).await;
        return;
    };

    if !is_valid_buzzer_id(buzzer_id) {
        warn!(id = buzzer_id, "invalid buzzer id");
        let _ = outbound_tx.send(Message::Close(None));
        finalize(writer_task, outbound_tx).await;
        return;
    }

    let buzzer_id = buzzer_id.to_string();
    state.buzzers().insert(
        buzzer_id.clone(),
        BuzzerConnection {
            id: buzzer_id.clone(),
            tx: outbound_tx.clone(),
        },
    );

    info!(id = %buzzer_id, "buzzer connected");

    send_message_to_websocket(
        &outbound_tx,
        &BuzzerAck {
            id: buzzer_id.clone(),
            status: "ready".to_string(),
        },
        "buzzer ack",
    );

    while let Some(message) = receiver.next().await {
        match message {
            Ok(Message::Text(text)) => {
                info!(id = %buzzer_id, payload = %text, "received buzzer message");

                match serde_json::from_str::<BuzzerInboundMessage>(&text) {
                    Ok(BuzzerInboundMessage::Buzz { id }) => {
                        let res = if id == buzzer_id {
                            handle_buzz(&state, &id).await
                        } else {
                            Err(ServiceError::InvalidState(format!(
                                "Buzz ignored: mismatched ID (expected {buzzer_id}, got {id})"
                            )))
                        };

                        let can_answer = res.is_ok();
                        if let Err(err) = res {
                            warn!(
                                error = %err,
                                "Error while handling buzz (form ID {id})",
                            );
                        };
                        send_message_to_websocket(
                            &outbound_tx,
                            &BuzzFeedback { id, can_answer },
                            "buzzer feedback",
                        );
                    }
                    Ok(BuzzerInboundMessage::Identification { .. }) => {
                        warn!(id = %buzzer_id, "ignoring duplicate identification message");
                    }
                    Ok(BuzzerInboundMessage::Unknown) | Err(_) => {
                        warn!(id = %buzzer_id, "unrecognised buzzer message");
                    }
                }
            }
            Ok(Message::Ping(payload)) => {
                let _ = outbound_tx.send(Message::Pong(payload));
            }
            Ok(Message::Close(frame)) => {
                info!(id = %buzzer_id, "buzzer closed");
                let _ = outbound_tx.send(Message::Close(frame));
                break;
            }
            Ok(Message::Binary(_)) => {}
            Ok(Message::Pong(_)) => {}
            Err(err) => {
                warn!(id = %buzzer_id, error = %err, "websocket error");
                break;
            }
        }
    }

    state.buzzers().remove(&buzzer_id);
    info!(id = %buzzer_id, "buzzer disconnected");

    finalize(writer_task, outbound_tx).await;
}

/// Serialize a payload and push it onto the provided WebSocket sender, logging failures.
pub fn send_message_to_websocket<T>(
    tx: &mpsc::UnboundedSender<Message>,
    value: &T,
    message_type: &str,
) where
    T: ?Sized + serde::Serialize,
{
    match serde_json::to_string(value) {
        Ok(payload) => {
            if tx.send(Message::Text(payload.into())).is_err() {
                warn!("websocket writer closed while sending {message_type}");
            }
        }
        Err(err) => warn!(error = %err, "failed to serialize {message_type}"),
    }
}

/// Process a buzz coming from a buzzer connection, returning whether the team can answer.
pub async fn handle_buzz(state: &SharedState, buzzer_id: &str) -> Result<(), ServiceError> {
    let player_known = {
        let guard = state.current_game().read().await;
        guard.as_ref().is_some_and(|game| {
            game.players
                .iter()
                .any(|player| player.buzzer_id == buzzer_id)
        })
    };

    if !player_known {
        return Err(ServiceError::InvalidState(format!(
            "Buzz ignored: unknown buzzer ID `{buzzer_id}`"
        )));
    }

    run_transition_with_broadcast(
        state,
        GameEvent::Pause(PauseKind::Buzz {
            id: buzzer_id.into(),
        }),
        move || async move { Ok(()) },
    )
    .await
}

/// Buzzer identifiers must be 12 lowercase hexadecimal characters with no separators.
fn is_valid_buzzer_id(value: &str) -> bool {
    value.len() == 12
        && value
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, 'a'..='f'))
}

/// Ensure the writer task winds down before we return from the socket handler.
async fn finalize(writer_task: JoinHandle<()>, outbound_tx: mpsc::UnboundedSender<Message>) {
    drop(outbound_tx);
    let _ = writer_task.await;
}
