use mongodb::bson::DateTime;
use uuid::Uuid;

use crate::{
    dao::game::GameRepository,
    dto::{
        admin::{
            ActionResponse, AnswerValidationRequest, CreateGameFromPlaylistRequest, FieldKind,
            FieldsFoundResponse, GameListItem, MarkFieldRequest, NextSongResponse,
            PlaylistListItem, ScoreAdjustmentRequest, ScoreUpdateResponse, StartGameResponse,
            StopGameResponse,
        },
        game::{CreateGameRequest, GameSummary, SongSummary},
    },
    error::ServiceError,
    services::{game_service, sse_events},
    state::{
        SharedState,
        game::{GameSession, PointField},
        state_machine::{FinishReason, GameEvent, GamePhase, GameRunningPhase, PauseKind},
    },
};

async fn mongo_repo(state: &SharedState) -> Result<GameRepository, ServiceError> {
    let Some(mongo) = state.mongo().await else {
        return Err(ServiceError::Degraded);
    };
    Ok(GameRepository::new(mongo))
}

async fn persist_current_game(state: &SharedState) -> Result<(), ServiceError> {
    let repository = mongo_repo(state).await?;
    let snapshot = {
        let guard = state.current_game().read().await;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| ServiceError::InvalidState("no active game".into()))?
    };
    repository.save(snapshot.into()).await?;
    Ok(())
}

async fn ensure_running_phase(state: &SharedState) -> Result<GameRunningPhase, ServiceError> {
    let phase = { state.game().read().await.phase() };
    match phase {
        GamePhase::GameRunning(sub) => Ok(sub),
        other => Err(ServiceError::InvalidState(format!(
            "operation requires running phase, current: {other:?}"
        ))),
    }
}

fn unwrap_current_game_mut(
    guard: &mut Option<GameSession>,
) -> Result<&mut GameSession, ServiceError> {
    guard
        .as_mut()
        .ok_or_else(|| ServiceError::InvalidState("no active game".into()))
}

fn song_summary(game: &GameSession, index: usize) -> Result<SongSummary, ServiceError> {
    let song_id = *game
        .playlist_song_order
        .get(index)
        .ok_or_else(|| ServiceError::InvalidState("song index out of bounds".into()))?;
    let song_entry = game
        .playlist
        .songs
        .get(&song_id)
        .ok_or_else(|| ServiceError::InvalidState("song not found in playlist".into()))?;
    Ok((song_id, song_entry.value().clone()).into())
}

pub async fn list_games(state: &SharedState) -> Result<Vec<GameListItem>, ServiceError> {
    let repository = mongo_repo(state).await?;
    let entries = repository.list_games().await?;
    Ok(entries
        .into_iter()
        .map(|(id, name)| GameListItem {
            id: id.to_string(),
            name,
        })
        .collect())
}

pub async fn list_playlists(state: &SharedState) -> Result<Vec<PlaylistListItem>, ServiceError> {
    let repository = mongo_repo(state).await?;
    let entries = repository.list_playlists().await?;
    Ok(entries
        .into_iter()
        .map(|(id, name)| PlaylistListItem {
            id: id.to_string(),
            name,
        })
        .collect())
}

pub async fn load_game(state: &SharedState, id: Uuid) -> Result<GameSummary, ServiceError> {
    sse_events::apply_and_broadcast_event(state, GameEvent::StartGame).await?;
    let summary = game_service::load_game(state, id).await?;
    Ok(summary)
}

pub async fn create_game(
    state: &SharedState,
    request: CreateGameRequest,
) -> Result<GameSummary, ServiceError> {
    sse_events::apply_and_broadcast_event(state, GameEvent::StartGame).await?;
    let summary = game_service::create_game(state, request).await?;
    Ok(summary)
}

pub async fn create_game_from_playlist(
    state: &SharedState,
    request: CreateGameFromPlaylistRequest,
) -> Result<GameSummary, ServiceError> {
    sse_events::apply_and_broadcast_event(state, GameEvent::StartGame).await?;
    let summary = game_service::create_game_from_playlist(state, request).await?;
    Ok(summary)
}

pub async fn start_game(state: &SharedState) -> Result<StartGameResponse, ServiceError> {
    sse_events::apply_and_broadcast_event(state, GameEvent::GameConfigured).await?;
    let song = {
        let mut guard = state.current_game().write().await;
        let game = unwrap_current_game_mut(&mut guard)?;
        if game.playlist_song_order.is_empty() {
            panic!("Error when starting game: list should not be empty here (checked before)")
        }
        game.current_song_index = Some(0);
        game.found_point_fields.clear();
        game.found_bonus_fields.clear();
        game.updated_at = DateTime::now();
        song_summary(game, 0)?
    };

    persist_current_game(state).await?;
    Ok(StartGameResponse { song })
}

pub async fn pause_game(state: &SharedState) -> Result<ActionResponse, ServiceError> {
    sse_events::apply_and_broadcast_event(state, GameEvent::Pause(PauseKind::Manual)).await?;
    Ok(ActionResponse {
        message: "paused".into(),
    })
}

pub async fn resume_game(state: &SharedState) -> Result<ActionResponse, ServiceError> {
    // First retrieve buzzer ID of current phase (if we are in pause by buzz)
    let buzzer_id = match state.game().read().await.phase() {
        GamePhase::GameRunning(GameRunningPhase::Paused(PauseKind::Buzz { id })) => Some(id),
        _ => None,
    };
    // Then apply the event and continue
    sse_events::apply_and_broadcast_event(state, GameEvent::ContinuePlaying).await?;
    if let Some(id) = buzzer_id {
        state.notify_buzzer_turn_finished(&id);
    }
    Ok(ActionResponse {
        message: "resumed".into(),
    })
}

pub async fn reveal(state: &SharedState) -> Result<ActionResponse, ServiceError> {
    // First retrieve buzzer ID of current phase (if we are in pause by buzz)
    let buzzer_id = match state.game().read().await.phase() {
        GamePhase::GameRunning(GameRunningPhase::Paused(PauseKind::Buzz { id })) => Some(id),
        _ => None,
    };
    // Then apply the event and continue
    sse_events::apply_and_broadcast_event(state, GameEvent::Reveal).await?;
    if let Some(id) = buzzer_id {
        state.notify_buzzer_turn_finished(&id);
    }
    Ok(ActionResponse {
        message: "revealed".into(),
    })
}

pub async fn next_song(state: &SharedState) -> Result<NextSongResponse, ServiceError> {
    let next_index = {
        let guard = state.current_game().read().await;
        let game = guard
            .as_ref()
            .ok_or_else(|| ServiceError::InvalidState("no active game".into()))?;
        let len = game.playlist_song_order.len();
        let current = game
            .current_song_index
            .ok_or_else(|| ServiceError::InvalidState("no active song".into()))?;
        if current + 1 < len {
            Some(current + 1)
        } else {
            None // playlist is finished
        }
    };

    if next_index.is_some() {
        sse_events::apply_and_broadcast_event(state, GameEvent::NextSong).await?;
    } else {
        sse_events::apply_and_broadcast_event(
            state,
            GameEvent::Finish(FinishReason::PlaylistCompleted),
        )
        .await?;
    }

    let next_song = {
        let mut guard = state.current_game().write().await;
        let game = unwrap_current_game_mut(&mut guard)?;
        game.current_song_index = next_index;
        game.found_point_fields.clear();
        game.found_bonus_fields.clear();
        game.updated_at = DateTime::now();
        if let Some(target_index) = next_index {
            Some(song_summary(game, target_index)?)
        } else {
            None
        }
    };

    persist_current_game(state).await?;

    Ok(NextSongResponse {
        finished: next_song.is_none(),
        song: next_song,
    })
}

pub async fn stop_game(state: &SharedState) -> Result<StopGameResponse, ServiceError> {
    sse_events::apply_and_broadcast_event(state, GameEvent::Finish(FinishReason::ManualStop))
        .await?;
    let teams = {
        let mut guard = state.current_game().write().await;
        let game = unwrap_current_game_mut(&mut guard)?;
        game.current_song_index = None;
        game.found_point_fields.clear();
        game.found_bonus_fields.clear();
        game.updated_at = DateTime::now();
        game.players.iter().cloned().map(Into::into).collect()
    };
    persist_current_game(state).await?;
    Ok(StopGameResponse { teams })
}

pub async fn end_game(state: &SharedState) -> Result<ActionResponse, ServiceError> {
    sse_events::apply_and_broadcast_event(state, GameEvent::EndGame).await?;
    {
        let mut guard = state.current_game().write().await;
        guard.take();
    }
    Ok(ActionResponse {
        message: "ended".into(),
    })
}

pub async fn mark_field_found(
    state: &SharedState,
    request: MarkFieldRequest,
) -> Result<FieldsFoundResponse, ServiceError> {
    let running_phase = ensure_running_phase(state).await?;
    if matches!(running_phase, GameRunningPhase::Prep) {
        return Err(ServiceError::InvalidState(
            "cannot mark fields during preparation".into(),
        ));
    }

    let (response, song_id) = {
        let mut guard = state.current_game().write().await;
        let game = unwrap_current_game_mut(&mut guard)?;
        let index = game
            .current_song_index
            .ok_or_else(|| ServiceError::InvalidState("no active song".into()))?;
        let song_id = *game
            .playlist_song_order
            .get(index)
            .ok_or_else(|| ServiceError::InvalidState("song index out of bounds".into()))?;
        let song = game
            .playlist
            .songs
            .get(&song_id)
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

        let response = FieldsFoundResponse {
            song_id: song_id.clone().to_string(),
            point_fields: game.found_point_fields.clone(),
            bonus_fields: game.found_bonus_fields.clone(),
        };

        (response, song_id)
    };

    sse_events::broadcast_fields_found(
        state,
        song_id,
        &response.point_fields,
        &response.bonus_fields,
    );

    Ok(response)
}

pub async fn validate_answer(
    state: &SharedState,
    request: AnswerValidationRequest,
) -> Result<ActionResponse, ServiceError> {
    let running_phase = ensure_running_phase(state).await?;
    match running_phase {
        GameRunningPhase::Paused(_) => {
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
    ensure_running_phase(state).await?;

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

fn ensure_field_exists(fields: &[PointField], field_key: &str) -> Result<(), ServiceError> {
    if fields.iter().any(|field| field.key == field_key) {
        Ok(())
    } else {
        Err(ServiceError::InvalidInput(format!(
            "field `{field_key}` does not exist for this song"
        )))
    }
}
