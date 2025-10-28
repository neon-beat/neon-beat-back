use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
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

    send_message_to_websocket(
        &outbound_tx,
        &BuzzerOutboundMessage {
            pattern: state.buzzer_pattern(BuzzerPatternPreset::WaitingForPairing),
        },
    );

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
                                Err(ServiceError::InvalidState(format!(
                                    "Buzz ignored: mismatched ID (expected {buzzer_id}, got {id})"
                                )))
                            };
                            if let Err(err) = res {
                                warn!(
                                    error = %err,
                                    "Error while handling buzz (form ID {id})",
                                );
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

/// Serialize a payload and push it onto the provided WebSocket sender, logging failures.
pub fn send_message_to_websocket<T>(tx: &mpsc::UnboundedSender<Message>, value: &T)
where
    T: ?Sized + serde::Serialize + std::fmt::Debug,
{
    match serde_json::to_string(value) {
        Ok(payload) => {
            if tx.send(Message::Text(payload.into())).is_err() {
                warn!("websocket writer closed while sending message `{value:?}`");
            }
        }
        Err(err) => warn!(error = %err, "failed to serialize the message `{value:?}`"),
    }
}

/// Send a pattern update to the buzzer associated with `team`.
pub fn send_pattern_to_team_buzzer(
    state: &SharedState,
    team_id: &Uuid,
    team: &Team,
    preset: BuzzerPatternPreset,
) -> Result<(), ServiceError> {
    let buzzer_id = team.buzzer_id.as_ref().ok_or_else(|| {
        ServiceError::InvalidState(format!("team `{team_id}` has no paired buzzer"))
    })?;
    send_pattern_to_buzzer(state, buzzer_id, preset)
}

/// Send a pattern update to a buzzer.
pub fn send_pattern_to_buzzer(
    state: &SharedState,
    buzzer_id: &String,
    preset: BuzzerPatternPreset,
) -> Result<(), ServiceError> {
    let tx = state
        .buzzers()
        .get(buzzer_id)
        .ok_or_else(|| {
            ServiceError::InvalidState(format!("buzzer `{buzzer_id}` is not connected"))
        })?
        .tx
        .clone();

    send_message_to_websocket(
        &tx,
        &BuzzerOutboundMessage {
            pattern: state.buzzer_pattern(preset),
        },
    );

    Ok(())
}

/// Process a buzz coming from a buzzer connection, returning whether the team can answer.
pub async fn handle_buzz(
    state: &SharedState,
    buzzer_id: &str,
    outbound_tx: &mpsc::UnboundedSender<Message>,
) -> Result<(), ServiceError> {
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
        _ => Err(ServiceError::InvalidState(
            "buzz events are ignored outside of running phases".into(),
        )),
    }
}

async fn handle_prep_ready_buzz(
    state: &SharedState,
    buzzer_id: &str,
    outbound_tx: &mpsc::UnboundedSender<Message>,
) -> Result<(), ServiceError> {
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
                send_message_to_websocket(
                    outbound_tx,
                    &BuzzerOutboundMessage {
                        pattern: state
                            .buzzer_pattern(BuzzerPatternPreset::Standby(new_team.color.clone())),
                    },
                );
                Ok(Some((game.id, team_id, new_team)))
            } else {
                Ok(None)
            }
        })
        .await?;

    if let Some((game_id, team_id, team)) = maybe_result {
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
) -> Result<(), ServiceError> {
    let pairing_session = state
        .pairing_session()
        .await
        .ok_or_else(|| ServiceError::InvalidState("pairing workflow lost session state".into()))?;
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

    send_message_to_websocket(
        outbound_tx,
        &BuzzerOutboundMessage {
            pattern: state.buzzer_pattern(BuzzerPatternPreset::Standby(team_color)),
        },
    );

    let pairing_progress =
        apply_pairing_update(state, PairingSessionUpdate::Assigned { team_id, roster })
            .await?
            .ok_or_else(|| {
                ServiceError::InvalidState("pairing target changed during update".into())
            })?;

    // Persist game metadata and modified teams separately for efficiency
    state.persist_current_game_without_teams().await?;
    for (tid, team) in modified_teams {
        state.persist_team(game_id, tid, team).await?;
    }

    sse_events::broadcast_pairing_assigned(state, team_id, buzzer_id);
    handle_pairing_progress(state, pairing_progress).await?;

    Ok(())
}

async fn handle_playing_buzz(state: &SharedState, buzzer_id: &str) -> Result<(), ServiceError> {
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
    .await?;
    state
        .with_current_game(|game| {
            game.teams
                .iter()
                .map(|(team_id, team)| {
                    let team_buzzer_id = team.buzzer_id.as_ref().ok_or_else(|| {
                        ServiceError::InvalidState(format!("team `{team_id}` has no paired buzzer"))
                    })?;
                    let preset = if team_buzzer_id == buzzer_id {
                        BuzzerPatternPreset::Answering(team.color.clone())
                    } else {
                        BuzzerPatternPreset::Waiting
                    };
                    send_pattern_to_buzzer(state, team_buzzer_id, preset)
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .await?;
    Ok(())
}

/// Ensure the writer task winds down before we return from the socket handler.
async fn finalize(writer_task: JoinHandle<()>, outbound_tx: mpsc::UnboundedSender<Message>) {
    drop(outbound_tx);
    let _ = writer_task.await;
}
