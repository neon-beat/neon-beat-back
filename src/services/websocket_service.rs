use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{info, warn};

use crate::{
    dto::ws::{BuzzerAck, BuzzerInboundMessage},
    state::{BuzzerConnection, SharedState},
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

    if let Ok(payload) = serde_json::to_string(&BuzzerAck {
        id: buzzer_id.clone(),
        status: "ready".to_string(),
    }) {
        let _ = outbound_tx.send(Message::Text(payload.into()));
    }

    while let Some(message) = receiver.next().await {
        match message {
            Ok(Message::Text(text)) => {
                info!(id = %buzzer_id, payload = %text, "received buzzer message");
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
