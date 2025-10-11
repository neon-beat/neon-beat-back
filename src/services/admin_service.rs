//! Business logic powering the admin REST routes. These helpers coordinate
//! Storage persistence, in-memory state updates, and state-machine transitions
//! while honouring the single-transition-at-a-time requirement.

use std::{sync::Arc, time::SystemTime};
use uuid::Uuid;

use crate::{
    dao::game::GameStore,
    dto::{
        admin::{
            ActionResponse, AnswerValidationRequest, CreateGameRequest, FieldKind,
            FieldsFoundResponse, GameListItem, MarkFieldRequest, NextSongResponse,
            PlaylistListItem, ScoreAdjustmentRequest, ScoreUpdateResponse, StartGameResponse,
            StopGameResponse,
        },
        game::{
            CreateGameWithPlaylistRequest, GameSummary, PlaylistInput, PlaylistSummary, SongSummary,
        },
        sse::TeamSummary,
    },
    error::ServiceError,
    services::{game_service, sse_events},
    state::{
        SharedState,
        game::{GameSession, PointField},
        state_machine::{FinishReason, GameEvent, GamePhase, GameRunningPhase, PauseKind},
        transitions::run_transition_with_broadcast,
    },
};

/// Obtain the configured game store or surface degraded mode.
async fn game_store(state: &SharedState) -> Result<Arc<dyn GameStore>, ServiceError> {
    state.game_store().await.ok_or(ServiceError::Degraded)
}

/// Persist the in-memory game session back into the configured store.
async fn persist_current_game(state: &SharedState) -> Result<(), ServiceError> {
    let store = game_store(state).await?;
    let snapshot = {
        let guard = state.current_game().read().await;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| ServiceError::InvalidState("no active game".into()))?
    };
    store.save_game(snapshot.into()).await?;
    Ok(())
}

/// Borrow the active game session mutably or produce an invalid-state error.
fn unwrap_current_game_mut(
    guard: &mut Option<GameSession>,
) -> Result<&mut GameSession, ServiceError> {
    guard
        .as_mut()
        .ok_or_else(|| ServiceError::InvalidState("no active game".into()))
}

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
    let store = game_store(state).await?;
    let entries = store.list_games().await?;
    Ok(entries
        .into_iter()
        .map(|(id, name)| GameListItem { id, name })
        .collect())
}

/// Return the playlists that can seed new games.
pub async fn list_playlists(state: &SharedState) -> Result<Vec<PlaylistListItem>, ServiceError> {
    let store = game_store(state).await?;
    let entries = store.list_playlists().await?;
    Ok(entries
        .into_iter()
        .map(|(id, name)| PlaylistListItem { id, name })
        .collect())
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
            request.players,
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
            request.players,
            request.playlist_id,
            None,
        )
        .await
    })
    .await
}

/// Move the admin-controlled game into the running phase and expose the first song.
pub async fn start_game(state: &SharedState) -> Result<StartGameResponse, ServiceError> {
    let song_summary = load_next_song(state, true)
        .await?
        .expect("Error during game start: no song found in playlist after transitionning the state (should not happen)");
    Ok(StartGameResponse { song: song_summary })
}

/// Pause gameplay manually through the admin controls.
pub async fn pause_game(state: &SharedState) -> Result<ActionResponse, ServiceError> {
    run_transition_with_broadcast(
        state,
        GameEvent::Pause(PauseKind::Manual),
        move || async move {
            Ok(ActionResponse {
                message: "paused".into(),
            })
        },
    )
    .await
}

/// Resume gameplay when an admin clears a pause.
pub async fn resume_game(state: &SharedState) -> Result<ActionResponse, ServiceError> {
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
    .await
}

/// Reveal the current song and conclude any outstanding buzz sequence.
pub async fn reveal(state: &SharedState) -> Result<ActionResponse, ServiceError> {
    run_transition_with_broadcast(state, GameEvent::Reveal, move || async move {
        if let GamePhase::GameRunning(GameRunningPhase::Paused(PauseKind::Buzz { id })) =
            state.state_machine_phase().await
        {
            state.notify_buzzer_turn_finished(&id)
        };
        Ok(ActionResponse {
            message: "revealed".into(),
        })
    })
    .await
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
    let (current_song_index, playlist_length) = {
        let guard = state.current_game().read().await;
        let game = guard
            .as_ref()
            .ok_or_else(|| ServiceError::InvalidState("no active game".into()))?;
        (game.current_song_index, game.playlist_song_order.len())
    };
    let next_song_index = if start {
        current_song_index
    } else {
        let next_song_index = current_song_index
            .map(|i| i + 1)
            .ok_or_else(|| ServiceError::InvalidState("no active song".into()))?;
        if next_song_index < playlist_length {
            Some(next_song_index)
        } else {
            None
        }
    };
    let event = if start {
        GameEvent::GameConfigured
    } else if next_song_index.is_some() {
        GameEvent::NextSong
    } else {
        GameEvent::Finish(FinishReason::PlaylistCompleted)
    };

    run_transition_with_broadcast(state, event, move || async move {
        let summary = {
            let mut guard = state.current_game().write().await;
            let game = unwrap_current_game_mut(&mut guard)?;
            game.current_song_index = next_song_index;
            game.found_point_fields.clear();
            game.found_bonus_fields.clear();
            game.updated_at = SystemTime::now();

            if let Some(index) = next_song_index {
                let (song_id, song) = game.get_song(index).ok_or_else(|| {
                    ServiceError::InvalidState("song not found in playlist".into())
                })?;
                Some((song_id, song).into())
            } else {
                None
            }
        };

        persist_current_game(state).await?;
        Ok(summary)
    })
    .await
}

/// Stop the running game early, capture standings, and persist them.
pub async fn stop_game(state: &SharedState) -> Result<StopGameResponse, ServiceError> {
    run_transition_with_broadcast(
        state,
        GameEvent::Finish(FinishReason::ManualStop),
        move || async move {
            let teams = {
                let mut guard = state.current_game().write().await;
                let game = unwrap_current_game_mut(&mut guard)?;
                game.current_song_index = None;
                game.found_point_fields.clear();
                game.found_bonus_fields.clear();
                game.updated_at = SystemTime::now();
                game.players
                    .iter()
                    .cloned()
                    .map(TeamSummary::from)
                    .collect::<Vec<_>>()
            };

            persist_current_game(state).await?;
            Ok(StopGameResponse { teams })
        },
    )
    .await
}

/// Clean up any remaining shared state after the game is complete.
pub async fn end_game(state: &SharedState) -> Result<ActionResponse, ServiceError> {
    run_transition_with_broadcast(state, GameEvent::EndGame, move || async move {
        let mut guard = state.current_game().write().await;
        guard.take();
        Ok(ActionResponse {
            message: "ended".into(),
        })
    })
    .await
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
    if matches!(running_phase, GameRunningPhase::Prep) {
        return Err(ServiceError::InvalidState(
            "cannot mark fields during preparation".into(),
        ));
    }

    let response = {
        let mut guard = state.current_game().write().await;
        let game = unwrap_current_game_mut(&mut guard)?;

        let index = game
            .current_song_index
            .ok_or_else(|| ServiceError::InvalidState("no active song".into()))?;
        let expected_song_id = *game
            .playlist_song_order
            .get(index)
            .ok_or_else(|| ServiceError::InvalidState("song index out of bounds".into()))?;
        if expected_song_id != request.song_id {
            return Err(ServiceError::InvalidInput(
                "song id does not match the current song".into(),
            ));
        }

        let song = game
            .playlist
            .songs
            .get(&request.song_id)
            .ok_or_else(|| ServiceError::InvalidState("song not found".into()))?;

        match request.kind {
            FieldKind::Point => {
                ensure_field_exists(&song.value().point_fields, &request.field_key)?;
                if !game.found_point_fields.contains(&request.field_key) {
                    game.found_point_fields.push(request.field_key.clone());
                }
            }
            FieldKind::Bonus => {
                ensure_field_exists(&song.value().bonus_fields, &request.field_key)?;
                if !game.found_bonus_fields.contains(&request.field_key) {
                    game.found_bonus_fields.push(request.field_key.clone());
                }
            }
        }

        FieldsFoundResponse {
            song_id: request.song_id,
            point_fields: game.found_point_fields.clone(),
            bonus_fields: game.found_bonus_fields.clone(),
        }
    };

    persist_current_game(state).await?;

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
    request: ScoreAdjustmentRequest,
) -> Result<ScoreUpdateResponse, ServiceError> {
    let phase = state.state_machine_phase().await;
    ensure_running_phase(phase)?;

    let updated_player = {
        let mut guard = state.current_game().write().await;
        let game = unwrap_current_game_mut(&mut guard)?;
        let player = game
            .players
            .iter_mut()
            .find(|p| p.buzzer_id == request.buzzer_id)
            .ok_or_else(|| ServiceError::NotFound("player not found".into()))?;
        player.score += request.delta;
        player.clone()
    };

    persist_current_game(state).await?;
    sse_events::broadcast_score_adjustment(state, updated_player.clone());

    Ok(ScoreUpdateResponse {
        buzzer_id: updated_player.buzzer_id,
        score: updated_player.score,
    })
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
