//! Business logic powering the admin REST routes. These helpers coordinate
//! Storage persistence, in-memory state updates, and state-machine transitions
//! while honouring the single-transition-at-a-time requirement.

use rand::{rng, seq::SliceRandom};
use std::time::SystemTime;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::{
    config::BuzzerPatternPreset,
    dto::{
        admin::{
            ActionResponse, AnswerValidationRequest, CreateGameRequest, CreateTeamRequest,
            FieldKind, FieldsFoundResponse, GameListItem, MarkFieldRequest, NextSongResponse,
            PlaylistListItem, ScoreAdjustmentRequest, ScoreUpdateResponse, StartGameResponse,
            StartPairingRequest, StopGameResponse, UpdateTeamRequest,
        },
        game::{
            CreateGameWithPlaylistRequest, GameSummary, PlaylistInput, PlaylistSummary,
            SongSummary, TeamInput, TeamSummary,
        },
        ws::BuzzerOutboundMessage,
    },
    error::ServiceError,
    services::{
        game_service,
        pairing::{PairingSessionUpdate, apply_pairing_update, handle_pairing_progress},
        sse_events,
        websocket_service::{send_message_to_websocket, send_pattern_to_team_buzzer},
    },
    state::{
        SharedState,
        game::{GameSession, PointField},
        state_machine::{
            FinishReason, GameEvent, GamePhase, GameRunningPhase, PairingSession, PauseKind,
            PrepStatus,
        },
        transitions::run_transition_with_broadcast,
    },
};

async fn ensure_prep_phase(state: &SharedState) -> Result<PrepStatus, ServiceError> {
    match state.state_machine_phase().await {
        GamePhase::GameRunning(GameRunningPhase::Prep(status)) => Ok(status),
        other => Err(ServiceError::InvalidState(format!(
            "operation requires prep phase, current phase {other:?}"
        ))),
    }
}

fn sanitize_optional_buzzer(input: Option<String>) -> Result<Option<String>, ServiceError> {
    match input {
        Some(value) => Ok(Some(game_service::sanitize_buzzer_id(&value)?)),
        None => Ok(None),
    }
}

fn assert_unique_buzzer(
    game: &GameSession,
    exclude: Option<Uuid>,
    buzzer_id: &str,
) -> Result<(), ServiceError> {
    if game
        .teams
        .iter()
        .any(|(id, team)| team.buzzer_id.as_deref() == Some(buzzer_id) && Some(*id) != exclude)
    {
        return Err(ServiceError::InvalidInput(format!(
            "duplicate buzzer id `{buzzer_id}` detected"
        )));
    }
    Ok(())
}

/// Borrow the active game session mutably or produce an invalid-state error.
/// Return the games persisted in storage for selection in the admin UI.
fn ensure_running_phase(phase: GamePhase) -> Result<GameRunningPhase, ServiceError> {
    match phase {
        GamePhase::GameRunning(sub) => Ok(sub),
        other => Err(ServiceError::InvalidState(format!(
            "operation requires running phase, current: {other:?}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Read-only projections
// ---------------------------------------------------------------------------

pub async fn list_games(state: &SharedState) -> Result<Vec<GameListItem>, ServiceError> {
    let store = state.require_game_store().await?;
    let game_entities = store.list_games().await?;

    let mut games_list = Vec::with_capacity(game_entities.len());
    for game in game_entities {
        let playlist = store
            .find_playlist(game.playlist_id)
            .await?
            .ok_or_else(|| {
                ServiceError::NotFound(format!("playlist {} not found", game.playlist_id))
            })?;
        games_list.push((game, playlist).try_into()?);
    }

    Ok(games_list)
}

pub async fn get_game_by_id(state: &SharedState, id: Uuid) -> Result<GameSummary, ServiceError> {
    let store = state.require_game_store().await?;

    let Some(game) = store.find_game(id).await? else {
        return Err(ServiceError::NotFound(format!("game `{id}` not found")));
    };

    let playlist = store
        .find_playlist(game.playlist_id)
        .await?
        .ok_or_else(|| {
            ServiceError::NotFound(format!("playlist {} not found", game.playlist_id))
        })?;

    let game_session: GameSession = (game, playlist).into();

    Ok(game_session.into())
}

/// Return the playlists that can seed new games.
pub async fn list_playlists(state: &SharedState) -> Result<Vec<PlaylistListItem>, ServiceError> {
    let store = state.require_game_store().await?;
    let entries = store.list_playlists().await?;
    Ok(entries
        .into_iter()
        .map(|(id, name)| PlaylistListItem { id, name })
        .collect())
}

pub async fn delete_game(state: &SharedState, id: Uuid) -> Result<(), ServiceError> {
    let current_game_id = state.read_current_game(|game| game.map(|g| g.id)).await;

    if current_game_id == Some(id) {
        if !matches!(state.state_machine_phase().await, GamePhase::Idle) {
            return Err(ServiceError::InvalidState(
                "cannot delete a game that is currently running".into(),
            ));
        }

        state
            .with_current_game_slot_mut(|slot| {
                slot.take();
            })
            .await;
    }

    let store = state.game_store().await.ok_or(ServiceError::Degraded)?;
    let deleted = store.delete_game(id).await?;
    if deleted {
        Ok(())
    } else {
        Err(ServiceError::NotFound(format!("game `{id}` not found")))
    }
}

/// Create and persist a reusable playlist definition on behalf of admins.
pub async fn create_playlist(
    state: &SharedState,
    request: PlaylistInput,
) -> Result<PlaylistSummary, ServiceError> {
    let (summary, _playlist) = game_service::create_playlist(state, request).await?;
    Ok(summary)
}

// ---------------------------------------------------------------------------
// Game bootstrap / lifecycle operations
// ---------------------------------------------------------------------------

/// Load a persisted game, apply the appropriate SSE event and return the summary.
pub async fn load_game(state: &SharedState, id: Uuid) -> Result<GameSummary, ServiceError> {
    run_transition_with_broadcast(state, GameEvent::StartGame, move || async move {
        game_service::load_game(state, id).await
    })
    .await
}

/// Create a new game definition on behalf of admins.
pub async fn create_game(
    state: &SharedState,
    request: CreateGameWithPlaylistRequest,
) -> Result<GameSummary, ServiceError> {
    run_transition_with_broadcast(state, GameEvent::StartGame, move || async move {
        let (_playlist_summary, playlist_model) =
            game_service::create_playlist(state, request.playlist).await?;
        game_service::create_game(
            state,
            request.name,
            request.teams,
            playlist_model.id,
            Some(playlist_model),
        )
        .await
    })
    .await
}

/// Create a game from a stored playlist template.
pub async fn create_game_from_playlist(
    state: &SharedState,
    request: CreateGameRequest,
) -> Result<GameSummary, ServiceError> {
    run_transition_with_broadcast(state, GameEvent::StartGame, move || async move {
        game_service::create_game(
            state,
            request.name,
            request.teams,
            request.playlist_id,
            None,
        )
        .await
    })
    .await
}

/// Move the admin-controlled game into the running phase and expose the first song.
pub async fn start_game(
    state: &SharedState,
    shuffle: bool,
) -> Result<StartGameResponse, ServiceError> {
    if let GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Ready)) =
        state.state_machine_phase().await
    {
        state
            .with_current_game(|game| {
                if game.teams.is_empty() {
                    return Err(ServiceError::InvalidInput(
                        "cannot start a game without at least one team".into(),
                    ));
                }

                if !state.all_teams_paired(&game.teams) {
                    return Err(ServiceError::InvalidState(
                        "cannot start game while unpaired teams remain".into(),
                    ));
                }

                if !state.buzzers().iter().all(|r| {
                    game.teams.iter().any(|(_, t)| {
                        t.buzzer_id
                            .as_ref()
                            .map(|id| id == r.key())
                            .unwrap_or(false)
                    })
                }) {
                    warn!("Some buzzers are not paired to any team while starting the game");
                }

                Ok(())
            })
            .await?;
    }

    let shuffled = if shuffle {
        state
            .with_current_game_mut(|game| {
                // Shuffle only if the playlist has not started or was completed
                if matches!(game.current_song_index, None | Some(0)) {
                    if game.playlist_song_order.len() > 1 {
                        let mut rng = rng();
                        game.playlist_song_order.shuffle(&mut rng);
                        game.updated_at = SystemTime::now();
                        Ok(Some(game.clone()))
                    } else {
                        Ok(None)
                    }
                } else {
                    Ok(None)
                }
            })
            .await?
    } else {
        None
    };

    if let Some(snapshot) = shuffled {
        state.persist_current_game().await?;
        sse_events::broadcast_game_session(state, &snapshot);
    }

    let song_summary = load_next_song(state, true)
        .await?
        .expect("Error during game start: no song found in playlist after transitionning the state (should not happen)");
    Ok(StartGameResponse { song: song_summary })
}

/// Pause gameplay manually through the admin controls.
pub async fn pause_game(state: &SharedState) -> Result<ActionResponse, ServiceError> {
    let result = run_transition_with_broadcast(
        state,
        GameEvent::Pause(PauseKind::Manual),
        move || async move {
            Ok(ActionResponse {
                message: "paused".into(),
            })
        },
    )
    .await?;
    state
        .with_current_game(|game| {
            game.teams
                .iter()
                .map(|(team_id, team)| {
                    send_pattern_to_team_buzzer(
                        state,
                        team_id,
                        team,
                        BuzzerPatternPreset::Waiting,
                        "waiting",
                    )
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .await?;
    Ok(result)
}

/// Resume gameplay when an admin clears a pause.
pub async fn resume_game(state: &SharedState) -> Result<ActionResponse, ServiceError> {
    let result =
        run_transition_with_broadcast(state, GameEvent::ContinuePlaying, move || async move {
            if let GamePhase::GameRunning(GameRunningPhase::Paused(PauseKind::Buzz { id })) =
                state.state_machine_phase().await
            {
                state.notify_buzzer_turn_finished(&id)
            };
            Ok(ActionResponse {
                message: "resumed".into(),
            })
        })
        .await?;
    state
        .with_current_game(|game| {
            game.teams
                .iter()
                .map(|(team_id, team)| {
                    send_pattern_to_team_buzzer(
                        state,
                        team_id,
                        team,
                        BuzzerPatternPreset::Playing(team.color.clone()),
                        "playing",
                    )
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .await?;
    Ok(result)
}

/// Reveal the current song and conclude any outstanding buzz sequence.
pub async fn reveal(state: &SharedState) -> Result<ActionResponse, ServiceError> {
    let result = run_transition_with_broadcast(state, GameEvent::Reveal, move || async move {
        if let GamePhase::GameRunning(GameRunningPhase::Paused(PauseKind::Buzz { id })) =
            state.state_machine_phase().await
        {
            state.notify_buzzer_turn_finished(&id)
        };

        state
            .with_current_game_mut(|game| {
                game.current_song_found = true;
                game.updated_at = SystemTime::now();
                Ok(())
            })
            .await?;

        state.persist_current_game().await?;

        Ok(ActionResponse {
            message: "revealed".into(),
        })
    })
    .await?;
    state
        .with_current_game(|game| {
            game.teams
                .iter()
                .map(|(team_id, team)| {
                    send_pattern_to_team_buzzer(
                        state,
                        team_id,
                        team,
                        BuzzerPatternPreset::Standby(team.color.clone()),
                        "standby",
                    )
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .await?;
    Ok(result)
}

/// Advance to the next song or finish the playlist when exhausted.
pub async fn next_song(state: &SharedState) -> Result<NextSongResponse, ServiceError> {
    let next_song_summary = load_next_song(state, false).await?;
    let response = NextSongResponse {
        finished: next_song_summary.is_none(),
        song: next_song_summary,
    };
    Ok(response)
}

async fn load_next_song(
    state: &SharedState,
    start: bool,
) -> Result<Option<SongSummary>, ServiceError> {
    let (current_song_index, playlist_length, current_song_found) = state
        .with_current_game(|game| {
            Ok((
                game.current_song_index,
                game.playlist_song_order.len(),
                game.current_song_found,
            ))
        })
        .await?;
    let next_song_index: Option<usize> = if start && !current_song_found {
        current_song_index.or(Some(0)) // "New Game +" if playlist was completed in the previous session
    } else {
        let next_song_index = current_song_index
            .ok_or_else(|| ServiceError::InvalidState("no active song: playlist is over".into()))?
            + 1;
        if next_song_index < playlist_length {
            Some(next_song_index)
        } else {
            if start {
                Some(0) // "New Game +" if playlist was completed in the previous session
            } else {
                None
            }
        }
    };
    let event = if start {
        GameEvent::GameConfigured
    } else if next_song_index.is_some() {
        GameEvent::NextSong
    } else {
        GameEvent::Finish(FinishReason::PlaylistCompleted)
    };

    let result = run_transition_with_broadcast(state, event, move || async move {
        let summary = state
            .with_current_game_mut(|game| {
                if game.current_song_index != next_song_index {
                    game.found_point_fields.clear();
                    game.found_bonus_fields.clear();
                }
                game.current_song_index = next_song_index;
                game.current_song_found = false;
                game.updated_at = SystemTime::now();

                if let Some(index) = next_song_index {
                    let (song_id, song) = game.get_song(index).ok_or_else(|| {
                        ServiceError::InvalidState("song not found in playlist".into())
                    })?;
                    Ok(Some((song_id, song).into()))
                } else {
                    Ok(None)
                }
            })
            .await?;

        state.persist_current_game().await?;
        Ok(summary)
    })
    .await?;
    if next_song_index.is_some() {
        state
            .with_current_game(|game| {
                game.teams
                    .iter()
                    .map(|(team_id, team)| {
                        send_pattern_to_team_buzzer(
                            state,
                            team_id,
                            team,
                            BuzzerPatternPreset::Playing(team.color.clone()),
                            "playing",
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .await?;
    };
    Ok(result)
}

/// Stop the running game early, capture standings, and persist them.
pub async fn stop_game(state: &SharedState) -> Result<StopGameResponse, ServiceError> {
    run_transition_with_broadcast(
        state,
        GameEvent::Finish(FinishReason::ManualStop),
        move || async move {
            let teams = state
                .with_current_game(|game| {
                    Ok(game
                        .teams
                        .iter()
                        .map(|(id, team)| (*id, team.clone()))
                        .map(Into::into)
                        .collect())
                })
                .await?;
            Ok(StopGameResponse { teams })
        },
    )
    .await
}

/// Clean up any remaining shared state after the game is complete.
pub async fn end_game(state: &SharedState) -> Result<ActionResponse, ServiceError> {
    let response = run_transition_with_broadcast(state, GameEvent::EndGame, move || async move {
        state
            .with_current_game_slot_mut(|slot| {
                slot.take();
            })
            .await;
        Ok(ActionResponse {
            message: "ended".into(),
        })
    })
    .await?;
    state.buzzers().iter().for_each(|connection| {
        let tx = connection.tx.clone();
        drop(connection);
        send_message_to_websocket(
            &tx,
            &BuzzerOutboundMessage {
                pattern: state.buzzer_pattern(BuzzerPatternPreset::WaitingForPairing),
            },
            "waiting for pairing",
        );
    });
    Ok(response)
}

// ---------------------------------------------------------------------------
// Gameplay adjustments that do not alter the state machine
// ---------------------------------------------------------------------------

/// Register a discovered field and broadcast the updated state to clients.
pub async fn mark_field_found(
    state: &SharedState,
    request: MarkFieldRequest,
) -> Result<FieldsFoundResponse, ServiceError> {
    let phase = state.state_machine_phase().await;
    let running_phase = ensure_running_phase(phase)?;
    if matches!(running_phase, GameRunningPhase::Prep(_)) {
        return Err(ServiceError::InvalidState(
            "cannot mark fields during preparation".into(),
        ));
    }

    let MarkFieldRequest {
        song_id,
        field_key,
        kind,
    } = request;

    let response = state
        .with_current_game_mut(|game| {
            let index = game.current_song_index.ok_or_else(|| {
                ServiceError::InvalidState("no active song: playlist is over".into())
            })?;
            let expected_song_id = *game
                .playlist_song_order
                .get(index)
                .ok_or_else(|| ServiceError::InvalidState("song index out of bounds".into()))?;
            if expected_song_id != song_id {
                return Err(ServiceError::InvalidInput(
                    "song id does not match the current song".into(),
                ));
            }

            let song = game
                .playlist
                .songs
                .get(&song_id)
                .ok_or_else(|| ServiceError::InvalidState("song not found".into()))?;

            match kind {
                FieldKind::Point => {
                    ensure_field_exists(&song.point_fields, &field_key)?;
                    if !game.found_point_fields.contains(&field_key) {
                        game.found_point_fields.push(field_key.clone());
                    }
                }
                FieldKind::Bonus => {
                    ensure_field_exists(&song.bonus_fields, &field_key)?;
                    if !game.found_bonus_fields.contains(&field_key) {
                        game.found_bonus_fields.push(field_key.clone());
                    }
                }
            }

            Ok(FieldsFoundResponse {
                song_id,
                point_fields: game.found_point_fields.clone(),
                bonus_fields: game.found_bonus_fields.clone(),
            })
        })
        .await?;

    state.persist_current_game().await?;

    sse_events::broadcast_fields_found(
        state,
        response.song_id,
        &response.point_fields,
        &response.bonus_fields,
    );

    Ok(response)
}

/// Apply answer validation decisions while the game is paused on a buzz.
pub async fn validate_answer(
    state: &SharedState,
    request: AnswerValidationRequest,
) -> Result<ActionResponse, ServiceError> {
    match state.state_machine_phase().await {
        GamePhase::GameRunning(GameRunningPhase::Paused(_)) => {
            sse_events::broadcast_answer_validation(state, request.valid);
            Ok(ActionResponse {
                message: "answered".into(),
            })
        }
        other => Err(ServiceError::InvalidState(format!(
            "cannot validate answer while in phase {other:?}"
        ))),
    }
}

pub async fn adjust_score(
    state: &SharedState,
    team_id: Uuid,
    request: ScoreAdjustmentRequest,
) -> Result<ScoreUpdateResponse, ServiceError> {
    let phase = state.state_machine_phase().await;
    ensure_running_phase(phase)?;

    let ScoreAdjustmentRequest { delta } = request;

    let updated_team = state
        .with_current_game_mut(|game| {
            let team = game
                .teams
                .get_mut(&team_id)
                .ok_or_else(|| ServiceError::NotFound("team not found".into()))?;
            team.score += delta;
            Ok(team.clone())
        })
        .await?;

    state.persist_current_game().await?;
    let score = updated_team.score;
    sse_events::broadcast_score_adjustment(state, team_id, updated_team);

    Ok(ScoreUpdateResponse { team_id, score })
}

/// Create a new team during the prep phase, automatically assigning an unused color from colors set when
/// one is not provided.
pub async fn create_team(
    state: &SharedState,
    request: CreateTeamRequest,
) -> Result<TeamSummary, ServiceError> {
    let prep_status = ensure_prep_phase(state).await?;
    if matches!(prep_status, PrepStatus::Pairing(_)) {
        return Err(ServiceError::InvalidState(
            "cannot modify teams during active pairing".into(),
        ));
    }

    let CreateTeamRequest(TeamInput {
        name,
        buzzer_id: buzzer_input,
        score,
        color: color_input,
    }) = request;

    if name.trim().is_empty() {
        return Err(ServiceError::InvalidInput(
            "team name must not be empty".into(),
        ));
    }

    let buzzer_id = sanitize_optional_buzzer(buzzer_input.unwrap_or_default())?;
    let config = state.config();

    let summary = state
        .with_current_game_mut(move |game| {
            if let Some(ref buzzer) = buzzer_id {
                assert_unique_buzzer(game, None, buzzer)?;
            }
            let (team_id, team) = game.add_team(
                config.as_ref(),
                Some(name),
                buzzer_id,
                score,
                color_input.map(Into::into),
            );
            Ok(TeamSummary::from((team_id, team)))
        })
        .await?;

    state.persist_current_game().await?;
    sse_events::broadcast_team_created(state, summary.clone());

    Ok(summary)
}

/// Update team metadata (name, buzzer, score) while in prep phase.
pub async fn update_team(
    state: &SharedState,
    team_id: Uuid,
    request: UpdateTeamRequest,
) -> Result<TeamSummary, ServiceError> {
    let UpdateTeamRequest(TeamInput {
        name,
        buzzer_id,
        score,
        color,
    }) = request;

    let prep_status = ensure_prep_phase(state).await?;
    if matches!(prep_status, PrepStatus::Pairing(_)) {
        return Err(ServiceError::InvalidState(
            "cannot modify teams during active pairing".into(),
        ));
    }

    if name.trim().is_empty() {
        return Err(ServiceError::InvalidInput(
            "team name must not be empty".into(),
        ));
    }

    let buzzer_update = buzzer_id.map(sanitize_optional_buzzer).transpose()?;

    let summary = state
        .with_current_game_mut(move |game| {
            if let Some(Some(ref buzzer)) = buzzer_update {
                assert_unique_buzzer(game, Some(team_id), buzzer)?;
            }

            let team = game
                .teams
                .get_mut(&team_id)
                .ok_or_else(|| ServiceError::NotFound(format!("team `{team_id}` not found")))?;

            team.name = name;
            if let Some(buzzer) = buzzer_update.clone() {
                team.buzzer_id = buzzer;
            }
            if let Some(new_score) = score {
                team.score = new_score;
            }
            if let Some(color_update) = color {
                team.color = color_update.into();
            }

            Ok(TeamSummary::from((team_id, team.clone())))
        })
        .await?;

    state.persist_current_game().await?;
    sse_events::broadcast_team_updated(state, summary.clone());

    Ok(summary)
}

/// Delete an existing team while in prep mode.
pub async fn delete_team(state: &SharedState, team_id: Uuid) -> Result<(), ServiceError> {
    let prep_status = ensure_prep_phase(state).await?;

    let roster = state
        .with_current_game_mut(move |game| {
            if game.teams.shift_remove(&team_id).is_none() {
                return Err(ServiceError::NotFound(format!(
                    "team `{team_id}` not found"
                )));
            }

            Ok(game.teams.clone())
        })
        .await?;

    let pairing_progress = match prep_status {
        PrepStatus::Ready => None,
        PrepStatus::Pairing(_) => {
            apply_pairing_update(state, PairingSessionUpdate::Deleted { team_id, roster }).await?
        }
    };

    state.persist_current_game().await?;
    sse_events::broadcast_team_deleted(state, team_id);
    if let Some(pairing_progress) = pairing_progress {
        handle_pairing_progress(state, pairing_progress).await?;
    } else {
        debug!(
            deleted_team_id = %team_id,
            "Pairing did not update (either PrepStatus::Ready or pairing_team_id != deleted_team_id)"
        );
    }

    Ok(())
}

/// Begin a pairing workflow for assigning buzzers to teams.
pub async fn start_pairing(
    state: &SharedState,
    request: StartPairingRequest,
) -> Result<(), ServiceError> {
    match ensure_prep_phase(state).await? {
        PrepStatus::Ready => {}
        PrepStatus::Pairing(_) => {
            return Err(ServiceError::InvalidState(
                "pairing is already in progress".into(),
            ));
        }
    }

    let first_team_id = request.first_team_id;

    let snapshot = state
        .with_current_game(|game| {
            if !game.teams.contains_key(&first_team_id) {
                return Err(ServiceError::NotFound(format!(
                    "team `{first_team_id}` not found"
                )));
            }

            Ok(game.teams.clone())
        })
        .await?;

    let session = PairingSession {
        pairing_team_id: first_team_id,
        snapshot,
    };

    run_transition_with_broadcast(
        state,
        GameEvent::PairingStarted(session),
        move || async move { Ok(()) },
    )
    .await?;

    state.persist_current_game().await?;
    sse_events::broadcast_pairing_waiting(state, first_team_id);

    Ok(())
}

/// Abort an active pairing workflow and restore the previous roster.
pub async fn abort_pairing(state: &SharedState) -> Result<Vec<TeamSummary>, ServiceError> {
    match ensure_prep_phase(state).await? {
        PrepStatus::Pairing(_) => {}
        PrepStatus::Ready => {
            return Err(ServiceError::InvalidState(
                "no pairing session is active".into(),
            ));
        }
    }

    let roster =
        run_transition_with_broadcast(state, GameEvent::PairingFinished, move || async move {
            let session = state
                .pairing_session()
                .await
                .ok_or_else(|| ServiceError::InvalidState("no pairing session is active".into()))?;
            let snapshot = session.snapshot;
            state
                .with_current_game_mut(move |game| {
                    game.teams = snapshot;
                    Ok(game.teams.clone())
                })
                .await
        })
        .await?;

    state.persist_current_game().await?;
    let teams = roster.clone().into_iter().map(Into::into).collect();
    sse_events::broadcast_pairing_restored(state, roster);

    Ok(teams)
}

/// Validate that the requested field is part of the song definition.
fn ensure_field_exists(fields: &[PointField], field_key: &str) -> Result<(), ServiceError> {
    if fields.iter().any(|field| field.key == field_key) {
        Ok(())
    } else {
        Err(ServiceError::InvalidInput(format!(
            "field `{field_key}` does not exist for this song"
        )))
    }
}
