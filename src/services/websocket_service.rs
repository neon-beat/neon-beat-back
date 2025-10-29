use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use thiserror::Error;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    config::BuzzerPatternPreset,
    dto::{
        game::TeamSummary,
        ws::{BuzzerInboundMessage, BuzzerOutboundMessage},
    },
    error::ServiceError,
    services::{
        pairing::{PairingSessionUpdate, apply_pairing_update, handle_pairing_progress},
        sse_events,
    },
    state::{
        BuzzerConnection, SharedState,
        game::Team,
        state_machine::{GameEvent, GamePhase, GameRunningPhase, PauseKind, PrepStatus},
        transitions::run_transition_with_broadcast,
    },
};

const IDENT_TIMEOUT: Duration = Duration::from_secs(10);

/// Internal error type for buzz handling operations.
///
/// This type represents errors that occur during WebSocket buzz processing,
/// distinct from `ServiceError` which is used for HTTP responses.
#[derive(Debug, Error)]
enum BuzzError {
    /// Writer channel closed - connection should be terminated immediately.
    #[error("connection closed")]
    ConnectionClosed,
    /// Buzzer ID in message doesn't match the connection's ID.
    #[error("buzz ignored: mismatched ID (expected {expected}, got {got})")]
    MismatchedId { expected: String, got: String },
    /// Buzz received outside of a running game phase.
    #[error("buzz events are ignored outside of running phases")]
    NotRunningPhase,
    /// Pairing session state was lost.
    #[error("pairing workflow lost session state")]
    PairingSessionLost,
    /// Pairing target changed during update operation.
    #[error("pairing target changed during update")]
    PairingTargetChanged,
    /// Buzzer ID is not associated with any team.
    #[error("buzz ignored: unknown buzzer ID `{0}`")]
    UnknownBuzzerId(String),
    /// Error from persistence or state management operations.
    #[error("service error: {0}")]
    Service(#[from] ServiceError),
}

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

    let inbound = match BuzzerInboundMessage::from_json_str(&initial_message) {
        Ok(message) => message,
        Err(err) => {
            warn!(error = %err, "failed to parse or validate buzzer message");
            let _ = outbound_tx.send(Message::Close(None));
            finalize(writer_task, outbound_tx).await;
            return;
        }
    };

    let BuzzerInboundMessage::Identification { id: buzzer_id } = inbound else {
        warn!("first message was not identification");
        let _ = outbound_tx.send(Message::Close(None));
        finalize(writer_task, outbound_tx).await;
        return;
    };

    state.buzzers().insert(
        buzzer_id.clone(),
        BuzzerConnection {
            id: buzzer_id.clone(),
            tx: outbound_tx.clone(),
        },
    );

    info!(id = %buzzer_id, "buzzer connected");

    // Determine which pattern to send on connection
    let initial_pattern = state
        .buzzer_last_patterns()
        .get(&buzzer_id)
        .map(|entry| {
            let pattern = entry.value().clone();
            info!(id = %buzzer_id, preset = ?pattern, "restoring last known pattern on reconnection");
            pattern
        })
        .unwrap_or(BuzzerPatternPreset::WaitingForPairing);

    // Send initial pattern - terminate on failure
    if send_pattern_to_buzzer_tx(&state, &buzzer_id, &outbound_tx, initial_pattern).is_err() {
        info!(id = %buzzer_id, "connection closed during initial pattern send, terminating");
        finalize(writer_task, outbound_tx).await;
        return;
    }

    while let Some(message) = receiver.next().await {
        match message {
            Ok(Message::Text(text)) => {
                info!(id = %buzzer_id, payload = %text, "received buzzer message");

                match BuzzerInboundMessage::from_json_str(&text) {
                    Ok(msg) => match msg {
                        BuzzerInboundMessage::Buzz { id } => {
                            let res = if id == buzzer_id {
                                handle_buzz(&state, &id, &outbound_tx).await
                            } else {
                                Err(BuzzError::MismatchedId {
                                    expected: buzzer_id.clone(),
                                    got: id,
                                })
                            };
                            if let Err(err) = res {
                                warn!(
                                    error = %err,
                                    "Error while handling buzz (from ID {buzzer_id})",
                                );
                                // If connection closed, terminate immediately
                                if matches!(err, BuzzError::ConnectionClosed) {
                                    info!(id = %buzzer_id, "Connection closed during buzz handling, terminating");
                                    break;
                                }
                            };
                        }
                        BuzzerInboundMessage::Identification { .. } => {
                            warn!(id = %buzzer_id, "ignoring duplicate identification message");
                        }
                    },
                    Err(err) => {
                        warn!(id = %buzzer_id, error = %err, "failed to parse or validate buzzer message");
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

/// Serialize a payload and push it onto the provided WebSocket sender.
///
/// Returns `Ok(())` if the message was successfully queued for sending or if
/// serialization failed (permanent error, no point retrying).
/// Returns `Err(BuzzError::ConnectionClosed)` if the writer channel is closed
/// (transient error, message should be retried when buzzer reconnects).
fn send_message_to_websocket<T>(
    tx: &mpsc::UnboundedSender<Message>,
    value: &T,
) -> Result<(), BuzzError>
where
    T: ?Sized + serde::Serialize + std::fmt::Debug,
{
    let payload = match serde_json::to_string(value) {
        Ok(p) => p,
        Err(err) => {
            // Serialization failure is a permanent error (bug in code)
            // Log and return Ok - no point storing as pending
            warn!(error = %err, "failed to serialize message `{value:?}` (permanent error, not retrying)");
            return Ok(());
        }
    };

    // Writer closed is a transient error - return error for caller to handle
    tx.send(Message::Text(payload.into()))
        .map_err(|_| BuzzError::ConnectionClosed)
}

/// Send a pattern update to the buzzer associated with `team`.
///
/// If the team has no paired buzzer or the buzzer is not connected,
/// logs a warning instead of returning an error.
pub fn send_pattern_to_team_buzzer(
    state: &SharedState,
    team_id: &Uuid,
    team: &Team,
    preset: BuzzerPatternPreset,
) {
    let Some(buzzer_id) = team.buzzer_id.as_ref() else {
        warn!(team_id = %team_id, "cannot send pattern: team has no paired buzzer");
        return;
    };
    send_pattern_to_buzzer(state, buzzer_id, preset);
}

/// Send a pattern update to a buzzer using its connection channel.
///
/// This function handles the actual sending and pattern tracking logic.
/// On success, the pattern is stored as the last known state for this buzzer.
/// On failure (writer closed), the pattern is still stored so it can be sent on reconnection,
/// and the buzzer is removed from the connected list.
///
/// Returns `Ok(())` if the message was sent successfully, or `Err(BuzzError::ConnectionClosed)`
/// if the writer channel is closed. The caller should handle connection cleanup if needed.
fn send_pattern_to_buzzer_tx(
    state: &SharedState,
    buzzer_id: &str,
    tx: &mpsc::UnboundedSender<Message>,
    preset: BuzzerPatternPreset,
) -> Result<(), BuzzError> {
    let message = BuzzerOutboundMessage {
        pattern: state.config().buzzer_pattern(preset.clone()),
    };

    let res = send_message_to_websocket(tx, &message);

    if res.is_err() {
        // Send failed (writer closed)
        warn!(buzzer_id = %buzzer_id, preset = ?preset, "send failed (writer closed), removing buzzer connection");
        state.buzzers().remove(buzzer_id);
    }

    // Store as last known pattern (if it was successful or not)
    state
        .buzzer_last_patterns()
        .insert(buzzer_id.to_string(), preset);
    res
}

/// Send a pattern update to a buzzer by ID.
///
/// Looks up the buzzer connection and delegates to `send_pattern_to_buzzer_tx`.
/// If the buzzer is not connected, the pattern is stored as the last known state
/// and will be sent when the buzzer reconnects.
fn send_pattern_to_buzzer(state: &SharedState, buzzer_id: &String, preset: BuzzerPatternPreset) {
    match state.buzzers().get(buzzer_id).map(|conn| conn.tx.clone()) {
        Some(tx) => {
            // Connected - send now (pattern stored automatically on success/failure)
            let _ = send_pattern_to_buzzer_tx(state, buzzer_id, &tx, preset);
        }
        None => {
            // Disconnected - store pattern for when buzzer reconnects
            warn!(buzzer_id = %buzzer_id, preset = ?preset, "buzzer disconnected, storing pattern for reconnection");
            state
                .buzzer_last_patterns()
                .insert(buzzer_id.clone(), preset);
        }
    }
}
/// Process a buzz coming from a buzzer connection, returning whether the team can answer.
async fn handle_buzz(
    state: &SharedState,
    buzzer_id: &str,
    outbound_tx: &mpsc::UnboundedSender<Message>,
) -> Result<(), BuzzError> {
    match state.state_machine_phase().await {
        GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Ready)) => {
            handle_prep_ready_buzz(state, buzzer_id, outbound_tx).await
        }
        GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Pairing(_))) => {
            handle_prep_pairing_buzz(state, buzzer_id, outbound_tx).await
        }
        GamePhase::GameRunning(GameRunningPhase::Playing) => {
            handle_playing_buzz(state, buzzer_id).await
        }
        _ => Err(BuzzError::NotRunningPhase),
    }
}

async fn handle_prep_ready_buzz(
    state: &SharedState,
    buzzer_id: &str,
    outbound_tx: &mpsc::UnboundedSender<Message>,
) -> Result<(), BuzzError> {
    let config = state.config();
    let maybe_result = state
        .with_current_game_mut(|game| {
            if let Some((&team_id, _)) = game
                .teams
                .iter()
                .find(|(_, team)| team.buzzer_id.as_deref() == Some(buzzer_id))
            {
                sse_events::broadcast_test_buzz(state, team_id);
                Ok(None)
            } else if state.all_teams_paired(&game.teams) {
                let (team_id, new_team) = game.add_team(
                    config.as_ref(),
                    None,
                    Some(buzzer_id.to_string()),
                    None,
                    None,
                );
                Ok(Some((game.id, team_id, new_team)))
            } else {
                Ok(None)
            }
        })
        .await?;

    if let Some((game_id, team_id, team)) = maybe_result {
        // If we can't notify the buzzer, abort - connection is dead
        send_pattern_to_buzzer_tx(
            state,
            buzzer_id,
            outbound_tx,
            BuzzerPatternPreset::Standby(team.color.clone()),
        )?;

        // Persist game metadata and the new team separately for efficiency
        state.persist_current_game_without_teams().await?;
        state.persist_team(game_id, team_id, team.clone()).await?;

        sse_events::broadcast_team_created(state, TeamSummary::from((team_id, team)));
    }
    Ok(())
}

/// Advance the pairing workflow when a buzzer is assigned during the prep pairing phase.
async fn handle_prep_pairing_buzz(
    state: &SharedState,
    buzzer_id: &str,
    outbound_tx: &mpsc::UnboundedSender<Message>,
) -> Result<(), BuzzError> {
    let pairing_session = state
        .pairing_session()
        .await
        .ok_or(BuzzError::PairingSessionLost)?;
    let team_id = pairing_session.pairing_team_id;

    let (game_id, roster, team_color, modified_teams) = state
        .with_current_game_mut(|game| {
            let mut modified_teams = Vec::new();

            let team_color = {
                let team = game
                    .teams
                    .get_mut(&team_id)
                    .ok_or_else(|| ServiceError::NotFound(format!("team `{team_id}` not found")))?;
                team.buzzer_id = Some(buzzer_id.to_string());
                modified_teams.push((team_id, team.clone()));
                team.color.clone()
            };

            for (id, team) in game.teams.iter_mut() {
                if *id != team_id && team.buzzer_id.as_deref() == Some(buzzer_id) {
                    team.buzzer_id = None;
                    modified_teams.push((*id, team.clone()));
                }
            }

            Ok((game.id, game.teams.clone(), team_color, modified_teams))
        })
        .await?;

    // If we can't notify the buzzer, abort - connection is dead
    send_pattern_to_buzzer_tx(
        state,
        buzzer_id,
        outbound_tx,
        BuzzerPatternPreset::Standby(team_color),
    )?;

    let pairing_progress =
        apply_pairing_update(state, PairingSessionUpdate::Assigned { team_id, roster })
            .await?
            .ok_or(BuzzError::PairingTargetChanged)?;

    // Persist game metadata and modified teams separately for efficiency
    state.persist_current_game_without_teams().await?;
    for (tid, team) in modified_teams {
        state.persist_team(game_id, tid, team).await?;
    }

    sse_events::broadcast_pairing_assigned(state, team_id, buzzer_id);
    handle_pairing_progress(state, pairing_progress).await?;

    Ok(())
}

async fn handle_playing_buzz(state: &SharedState, buzzer_id: &str) -> Result<(), BuzzError> {
    let team_known = state
        .read_current_game(|maybe| {
            maybe.is_some_and(|game| {
                game.teams
                    .iter()
                    .any(|(_, team)| team.buzzer_id.as_deref() == Some(buzzer_id))
            })
        })
        .await;

    if !team_known {
        return Err(BuzzError::UnknownBuzzerId(buzzer_id.to_string()));
    }

    run_transition_with_broadcast(
        state,
        GameEvent::Pause(PauseKind::Buzz {
            id: buzzer_id.into(),
        }),
        move || async move { Ok(()) },
    )
    .await?;
    let patterns_to_send = state
        .with_current_game(|game| {
            Ok(game
                .teams
                .iter()
                .filter_map(|(team_id, team)| {
                    if let Some(team_buzzer_id) = team.buzzer_id.as_ref() {
                        let preset = if team_buzzer_id == buzzer_id {
                            BuzzerPatternPreset::Answering(team.color.clone())
                        } else {
                            BuzzerPatternPreset::Waiting
                        };
                        Some((team_buzzer_id.clone(), preset))
                    } else {
                        warn!(team_id = %team_id, "cannot send pattern: team has no paired buzzer");
                        None
                    }
                })
                .collect::<Vec<_>>())
        })
        .await?;
    patterns_to_send
        .into_iter()
        .for_each(|(buzzer_id, preset)| send_pattern_to_buzzer(state, &buzzer_id, preset));
    Ok(())
}

/// Ensure the writer task winds down before we return from the socket handler.
async fn finalize(writer_task: JoinHandle<()>, outbound_tx: mpsc::UnboundedSender<Message>) {
    drop(outbound_tx);
    let _ = writer_task.await;
}
