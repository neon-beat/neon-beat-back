//! Business logic powering the admin REST routes. These helpers coordinate
//! Storage persistence, in-memory state updates, and state-machine transitions
//! while honouring the single-transition-at-a-time requirement.

use std::time::SystemTime;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::{
    config::BuzzerPatternPreset,
    dao::models::{QuestionEntity, QuestionsSequenceEntity},
    dto::{
        admin::{
            ActionResponse, AnswerFoundRequest, AnswersFoundResponse, CreateGameRequest,
            CreateTeamRequest, GameListItem, NextQuestionResponse, QuestionHintRequest,
            QuestionHintsResponse, QuestionValidationRequest, QuestionsSequenceListItem,
            ScoreAdjustmentRequest, ScoreUpdateResponse, StartGameResponse, StartPairingRequest,
            StopGameResponse, UpdateTeamRequest,
        },
        game::{
            CreateGameWithQuestionsSequenceRequest, GameSummary, QuestionSummary,
            QuestionsSequenceInput, QuestionsSequenceSummary, TeamInput, TeamSummary,
        },
    },
    error::ServiceError,
    services::{
        game_service,
        pairing::{PairingSessionUpdate, apply_pairing_update, handle_pairing_progress},
        sse_events,
        websocket_service::send_pattern_to_team_buzzer,
    },
    state::{
        SharedState,
        game::GameSession,
        state_machine::{
            FinishReason, GameEvent, GamePhase, GameRunningPhase, PairingSession, PauseKind,
            PrepStatus,
        },
        transitions::run_transition_with_broadcast,
    },
};

#[allow(deprecated)]
use crate::dto::admin::LegacyPlaylistListItem;

async fn ensure_prep_phase(state: &SharedState) -> Result<PrepStatus, ServiceError> {
    match state.state_machine_phase().await {
        GamePhase::GameRunning(GameRunningPhase::Prep(status)) => Ok(status),
        other => Err(ServiceError::InvalidState(format!(
            "operation requires prep phase, current phase {other:?}"
        ))),
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

/// Extract the running subphase or produce an invalid-state error.
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

/// List all games from storage with their basic information.
pub async fn list_games(state: &SharedState) -> Result<Vec<GameListItem>, ServiceError> {
    let store = state.require_game_store().await?;
    let game_entities = store.list_games().await?;

    let mut games_list = Vec::with_capacity(game_entities.len());
    for game in game_entities {
        let questions_sequence = store
            .find_questions_sequence(game.questions_sequence_id)
            .await?
            .ok_or_else(|| {
                ServiceError::NotFound(format!(
                    "questions sequence {} not found",
                    game.questions_sequence_id
                ))
            })?;
        games_list.push((game, questions_sequence).try_into()?);
    }

    Ok(games_list)
}

/// Retrieve a specific game by ID from storage.
pub async fn get_game_by_id(state: &SharedState, id: Uuid) -> Result<GameSummary, ServiceError> {
    let store = state.require_game_store().await?;

    let Some(game) = store.find_game(id).await? else {
        return Err(ServiceError::NotFound(format!("game `{id}` not found")));
    };

    let questions_sequence = store
        .find_questions_sequence(game.questions_sequence_id)
        .await?
        .ok_or_else(|| {
            ServiceError::NotFound(format!(
                "questions sequence {} not found",
                game.questions_sequence_id
            ))
        })?;

    let game_session: GameSession = (game, questions_sequence).into();

    Ok(game_session.try_into()?)
}

/// Return the questions sequences that can seed new games.
pub async fn list_questions_sequences(
    state: &SharedState,
) -> Result<Vec<QuestionsSequenceListItem>, ServiceError> {
    let store = state.require_game_store().await?;
    let entries = store.list_questions_sequences().await?;
    Ok(entries
        .into_iter()
        .map(|(id, name)| QuestionsSequenceListItem { id, name })
        .collect())
}

/// Return blindtest-only questions sequences through the deprecated playlist projection.
#[allow(deprecated)]
#[deprecated(
    since = "0.9.0",
    note = "Deprecated legacy playlist compatibility. Use list_questions_sequences instead."
)]
pub async fn list_legacy_playlists(
    state: &SharedState,
) -> Result<Vec<LegacyPlaylistListItem>, ServiceError> {
    let store = state.require_game_store().await?;
    let entries = store.list_questions_sequences().await?;
    let mut playlists = Vec::new();

    for (id, _name) in entries {
        let sequence = store.find_questions_sequence(id).await?.ok_or_else(|| {
            ServiceError::NotFound(format!("questions sequence `{id}` not found"))
        })?;

        if is_legacy_playlist_sequence(&sequence) {
            playlists.push(
                QuestionsSequenceListItem {
                    id: sequence.id,
                    name: sequence.name,
                }
                .into(),
            );
        }
    }

    Ok(playlists)
}

/// Return true when a persisted questions sequence can be exposed as a legacy playlist.
#[deprecated(
    since = "0.9.0",
    note = "Deprecated legacy playlist compatibility. Remove with legacy playlist route support."
)]
fn is_legacy_playlist_sequence(sequence: &QuestionsSequenceEntity) -> bool {
    !sequence.questions.is_empty()
        && sequence
            .questions
            .iter()
            .all(|question| matches!(question, QuestionEntity::BlindTest(_)))
}

/// Delete a game from storage by ID. Cannot delete a currently running game.
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

    let store = state.require_game_store().await?;
    let deleted = store.delete_game(id).await?;
    if deleted {
        Ok(())
    } else {
        Err(ServiceError::NotFound(format!("game `{id}` not found")))
    }
}

/// Create and persist a reusable questions sequence definition on behalf of admins.
pub async fn create_questions_sequence(
    state: &SharedState,
    request: QuestionsSequenceInput,
) -> Result<QuestionsSequenceSummary, ServiceError> {
    let (summary, _sequence) = game_service::create_questions_sequence(state, request).await?;
    Ok(summary)
}

// ---------------------------------------------------------------------------
// Game bootstrap / lifecycle operations
// ---------------------------------------------------------------------------

/// Load a persisted game, apply the appropriate SSE event and return the summary.
pub async fn load_game(
    state: &SharedState,
    id: Uuid,
    shuffle_questions: bool,
) -> Result<GameSummary, ServiceError> {
    run_transition_with_broadcast(state, GameEvent::StartGame, move || async move {
        game_service::load_game(state, id, shuffle_questions).await
    })
    .await
}

/// Create a new game definition on behalf of admins.
pub async fn create_game(
    state: &SharedState,
    request: CreateGameWithQuestionsSequenceRequest,
    shuffle_questions: bool,
) -> Result<GameSummary, ServiceError> {
    run_transition_with_broadcast(state, GameEvent::StartGame, move || async move {
        let (_sequence_summary, sequence_model) =
            game_service::create_questions_sequence(state, request.questions_sequence).await?;
        game_service::create_game(
            state,
            request.name,
            request.teams,
            sequence_model.id,
            Some(sequence_model),
            shuffle_questions,
        )
        .await
    })
    .await
}

/// Create a game from a stored questions sequence template.
pub async fn create_game_from_questions_sequence(
    state: &SharedState,
    request: CreateGameRequest,
    shuffle_questions: bool,
) -> Result<GameSummary, ServiceError> {
    run_transition_with_broadcast(state, GameEvent::StartGame, move || async move {
        game_service::create_game(
            state,
            request.name,
            request.teams,
            request.questions_sequence_id,
            None,
            shuffle_questions,
        )
        .await
    })
    .await
}

/// Move the admin-controlled game into the running phase and expose the first question.
pub async fn start_game(state: &SharedState) -> Result<StartGameResponse, ServiceError> {
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

    let question_summary = load_next_question(state, true).await?.ok_or_else(|| {
        ServiceError::InvalidState("no question found in sequence after starting the game".into())
    })?;
    Ok(StartGameResponse {
        question: question_summary,
    })
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
            game.teams.iter().for_each(|(team_id, team)| {
                send_pattern_to_team_buzzer(state, team_id, team, BuzzerPatternPreset::Waiting)
            });
            Ok(())
        })
        .await?;
    Ok(result)
}

/// Resume gameplay when an admin clears a pause.
pub async fn resume_game(state: &SharedState) -> Result<ActionResponse, ServiceError> {
    let result =
        run_transition_with_broadcast(state, GameEvent::ContinuePlaying, move || async move {
            Ok(ActionResponse {
                message: "resumed".into(),
            })
        })
        .await?;
    state
        .with_current_game(|game| {
            game.teams.iter().for_each(|(team_id, team)| {
                send_pattern_to_team_buzzer(
                    state,
                    team_id,
                    team,
                    BuzzerPatternPreset::Playing(team.color.clone()),
                )
            });
            Ok(())
        })
        .await?;
    Ok(result)
}

/// Reveal the current question and conclude any outstanding buzz sequence.
pub async fn reveal(state: &SharedState) -> Result<ActionResponse, ServiceError> {
    let result = run_transition_with_broadcast(state, GameEvent::Reveal, move || async move {
        state
            .with_current_game_mut(|game| {
                game.current_question_revealed = true;
                game.updated_at = SystemTime::now();
                Ok(())
            })
            .await?;

        state.persist_current_game_without_teams().await?;

        Ok(ActionResponse {
            message: "revealed".into(),
        })
    })
    .await?;
    state
        .with_current_game(|game| {
            game.teams.iter().for_each(|(team_id, team)| {
                send_pattern_to_team_buzzer(
                    state,
                    team_id,
                    team,
                    BuzzerPatternPreset::Standby(team.color.clone()),
                )
            });
            Ok(())
        })
        .await?;
    Ok(result)
}

/// Advance to the next question or finish the sequence when exhausted.
pub async fn next_question(state: &SharedState) -> Result<NextQuestionResponse, ServiceError> {
    let next_question_summary = load_next_question(state, false).await?;
    let response = NextQuestionResponse {
        finished: next_question_summary.is_none(),
        question: next_question_summary,
    };
    Ok(response)
}

/// Advance the current game to the next question and return its admin summary.
async fn load_next_question(
    state: &SharedState,
    start: bool,
) -> Result<Option<QuestionSummary>, ServiceError> {
    let (current_question_index, sequence_length, current_question_revealed) = state
        .with_current_game(|game| {
            Ok((
                game.current_question_index,
                game.question_order.len(),
                game.current_question_revealed,
            ))
        })
        .await?;
    let next_question_index: Option<usize> = if start && !current_question_revealed {
        current_question_index.or(Some(0)) // "New Game +" if sequence was completed in the previous session
    } else {
        let next_question_index = current_question_index.ok_or_else(|| {
            ServiceError::InvalidState("no active question: sequence is over".into())
        })? + 1;
        if next_question_index < sequence_length {
            Some(next_question_index)
        } else if start {
            Some(0) // "New Game +" if sequence was completed in the previous session
        } else {
            None // Sequence completed
        }
    };
    let event = if start {
        GameEvent::GameConfigured
    } else if next_question_index.is_some() {
        GameEvent::NextQuestion
    } else {
        GameEvent::Finish(FinishReason::QuestionsSequenceCompleted)
    };

    let result = run_transition_with_broadcast(state, event, move || async move {
        let summary = state
            .with_current_game_mut(|game| {
                if game.current_question_index != next_question_index {
                    game.found_answer_ids.clear();
                    game.revealed_hint_ids.clear();
                }
                game.current_question_index = next_question_index;
                game.current_question_revealed = false;
                game.updated_at = SystemTime::now();

                if let Some(index) = next_question_index {
                    let (question_id, question) = game.get_question(index).ok_or_else(|| {
                        ServiceError::InvalidState("question not found in sequence".into())
                    })?;
                    Ok(Some((question_id, question).into()))
                } else {
                    Ok(None)
                }
            })
            .await?;

        state.persist_current_game_without_teams().await?;
        Ok(summary)
    })
    .await?;
    if next_question_index.is_some() {
        state
            .with_current_game(|game| {
                game.teams.iter().for_each(|(team_id, team)| {
                    send_pattern_to_team_buzzer(
                        state,
                        team_id,
                        team,
                        BuzzerPatternPreset::Playing(team.color.clone()),
                    )
                });
                Ok(())
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
    let (response, teams) =
        run_transition_with_broadcast(state, GameEvent::EndGame, move || async move {
            // Extract teams before clearing the game
            let teams = state
                .read_current_game(|game| game.map(|g| g.teams.clone()).unwrap_or_default())
                .await;

            state
                .with_current_game_slot_mut(|slot| {
                    slot.take();
                })
                .await;

            Ok((
                ActionResponse {
                    message: "ended".into(),
                },
                teams,
            ))
        })
        .await?;

    // Send patterns only if transition succeeded
    for (team_id, team) in teams {
        send_pattern_to_team_buzzer(
            state,
            &team_id,
            &team,
            BuzzerPatternPreset::WaitingForPairing,
        );
    }

    Ok(response)
}

// ---------------------------------------------------------------------------
// Gameplay adjustments that do not alter the state machine
// ---------------------------------------------------------------------------

/// Register a discovered answer and broadcast the updated state to clients.
pub async fn mark_answer_found(
    state: &SharedState,
    request: AnswerFoundRequest,
) -> Result<AnswersFoundResponse, ServiceError> {
    let phase = state.state_machine_phase().await;
    let running_phase = ensure_running_phase(phase)?;
    if matches!(running_phase, GameRunningPhase::Prep(_)) {
        return Err(ServiceError::InvalidState(
            "cannot mark answers during preparation".into(),
        ));
    }

    let AnswerFoundRequest {
        question_id,
        answer_id,
    } = request;

    let response = state
        .with_current_game_mut(|game| {
            let question = current_question(game, question_id)?;

            if !question.has_answer(answer_id) {
                return Err(ServiceError::InvalidInput(format!(
                    "answer `{answer_id}` does not exist for this question"
                )));
            }

            if !game.found_answer_ids.contains(&answer_id) {
                match question {
                    crate::state::game::Question::BlindTest { .. }
                        if game.revealed_hint_ids.contains(&answer_id) => {
                        return Err(ServiceError::InvalidState(
                            "cannot mark an answer as found if its corresponding hint has already been revealed"
                                .into(),
                        ));
                        }
                    _ => game.found_answer_ids.push(answer_id),
                }
            }

            Ok(AnswersFoundResponse {
                question_id,
                answers_ids: game.found_answer_ids.clone(),
            })
        })
        .await?;

    state.persist_current_game_without_teams().await?;

    sse_events::broadcast_question_found_answers(
        state,
        response.question_id,
        &response.answers_ids,
    );

    Ok(response)
}

/// Reveal a hint and broadcast the updated state to clients.
pub async fn reveal_hint(
    state: &SharedState,
    request: QuestionHintRequest,
) -> Result<QuestionHintsResponse, ServiceError> {
    let phase = state.state_machine_phase().await;
    let running_phase = ensure_running_phase(phase)?;
    if matches!(running_phase, GameRunningPhase::Prep(_)) {
        return Err(ServiceError::InvalidState(
            "cannot reveal hints during preparation".into(),
        ));
    }

    let QuestionHintRequest {
        question_id,
        hint_id,
    } = request;

    let response = state
        .with_current_game_mut(|game| {
            let question = current_question(game, question_id)?;

            if !question.has_hint(hint_id) {
                return Err(ServiceError::InvalidInput(format!(
                    "hint `{hint_id}` does not exist for this question"
                )));
            }

            if !game.revealed_hint_ids.contains(&hint_id) {
                match question {
                    crate::state::game::Question::BlindTest { .. }
                        if game.found_answer_ids.contains(&hint_id) =>
                    {
                        return Err(ServiceError::InvalidState(
                            "cannot reveal a hint that corresponds to a found answer".into(),
                        ));
                    }
                    _ => game.revealed_hint_ids.push(hint_id),
                }
            }

            Ok(QuestionHintsResponse {
                question_id,
                hints_ids: game.revealed_hint_ids.clone(),
            })
        })
        .await?;

    state.persist_current_game_without_teams().await?;

    sse_events::broadcast_question_hints(state, response.question_id, &response.hints_ids);

    Ok(response)
}

/// Apply answer validation decisions while the game is paused on a buzz.
pub async fn submit_question_validation(
    state: &SharedState,
    request: QuestionValidationRequest,
) -> Result<ActionResponse, ServiceError> {
    match state.state_machine_phase().await {
        GamePhase::GameRunning(GameRunningPhase::Paused(_)) => {
            state
                .with_current_game(|game| {
                    current_question(game, request.question_id)?;
                    Ok(())
                })
                .await?;
            sse_events::broadcast_answer_validation(state, request.question_id, request.valid);
            Ok(ActionResponse {
                message: "answered".into(),
            })
        }
        other => Err(ServiceError::InvalidState(format!(
            "cannot validate answer while in phase {other:?}"
        ))),
    }
}

/// Adjust a team's score by a delta during gameplay.
pub async fn adjust_score(
    state: &SharedState,
    team_id: Uuid,
    request: ScoreAdjustmentRequest,
) -> Result<ScoreUpdateResponse, ServiceError> {
    let phase = state.state_machine_phase().await;
    ensure_running_phase(phase)?;

    let ScoreAdjustmentRequest { delta } = request;

    let (game_id, team_id, updated_team) = state
        .with_current_game_mut(|game| {
            let team = game
                .teams
                .get_mut(&team_id)
                .ok_or_else(|| ServiceError::NotFound("team not found".into()))?;
            team.score += delta;
            team.updated_at = std::time::SystemTime::now();
            Ok((game.id, team_id, team.clone()))
        })
        .await?;

    // Persist only the updated team, not the entire game
    state
        .persist_team(game_id, team_id, updated_team.clone())
        .await?;

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

    let buzzer_id = buzzer_input.unwrap_or_default();
    let config = state.config();

    let (game_id, team_id, team) = state
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
            Ok((game.id, team_id, team))
        })
        .await?;

    // Persist game metadata (including updated team_ids list) and the new team separately for efficiency
    state.persist_current_game_without_teams().await?;
    state.persist_team(game_id, team_id, team.clone()).await?;

    let summary = TeamSummary::from((team_id, team));
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

    let (game_id, updated_team) = state
        .with_current_game_mut(move |game| {
            if let Some(Some(ref buzzer)) = buzzer_id {
                assert_unique_buzzer(game, Some(team_id), buzzer)?;
            }

            let team = game
                .teams
                .get_mut(&team_id)
                .ok_or_else(|| ServiceError::NotFound(format!("team `{team_id}` not found")))?;

            team.name = name;
            if let Some(buzzer) = buzzer_id {
                team.buzzer_id = buzzer;
            }
            if let Some(new_score) = score {
                team.score = new_score;
            }
            if let Some(color_update) = color {
                team.color = color_update.into();
            }
            team.updated_at = std::time::SystemTime::now();

            Ok((game.id, team.clone()))
        })
        .await?;

    // Persist only the updated team, not the entire game
    state
        .persist_team(game_id, team_id, updated_team.clone())
        .await?;

    let summary = TeamSummary::from((team_id, updated_team));
    sse_events::broadcast_team_updated(state, summary.clone());

    Ok(summary)
}

/// Delete an existing team while in prep mode.
pub async fn delete_team(state: &SharedState, team_id: Uuid) -> Result<(), ServiceError> {
    let prep_status = ensure_prep_phase(state).await?;

    let (game_id, roster) = state
        .with_current_game_mut(move |game| {
            if game.teams.shift_remove(&team_id).is_none() {
                return Err(ServiceError::NotFound(format!(
                    "team `{team_id}` not found"
                )));
            }

            Ok((game.id, game.teams.clone()))
        })
        .await?;

    let pairing_progress = match prep_status {
        PrepStatus::Ready => None,
        PrepStatus::Pairing(_) => {
            apply_pairing_update(state, PairingSessionUpdate::Deleted { team_id, roster }).await?
        }
    };

    // Persist game metadata (updated team_ids list) and delete the team document separately for efficiency
    state.persist_current_game_without_teams().await?;
    state.delete_team(game_id, team_id).await?;

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

    state.persist_current_game_without_teams().await?;
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

    let (game_id, roster, modified_teams) =
        run_transition_with_broadcast(state, GameEvent::PairingFinished, move || async move {
            let session = state
                .pairing_session()
                .await
                .ok_or_else(|| ServiceError::InvalidState("no pairing session is active".into()))?;
            let snapshot = session.snapshot;

            state
                .with_current_game_mut(move |game| {
                    // Identify teams that changed during pairing by comparing buzzer_ids
                    let mut modified_teams = Vec::new();

                    for (team_id, snapshot_team) in snapshot.iter() {
                        if let Some(current_team) = game.teams.get(team_id) {
                            if current_team.buzzer_id != snapshot_team.buzzer_id {
                                modified_teams.push((*team_id, snapshot_team.clone()));
                            }
                        }
                    }

                    let game_id = game.id;
                    game.teams = snapshot.clone();
                    Ok((game_id, game.teams.clone(), modified_teams))
                })
                .await
        })
        .await?;

    // Persist game metadata and only the teams that were modified during pairing
    state.persist_current_game_without_teams().await?;
    for (team_id, team) in modified_teams {
        state.persist_team(game_id, team_id, team).await?;
    }

    let teams = roster.clone().into_iter().map(Into::into).collect();
    sse_events::broadcast_pairing_restored(state, roster);

    Ok(teams)
}

/// Return the current question after validating the client-provided identifier.
fn current_question(
    game: &GameSession,
    question_id: u32,
) -> Result<&crate::state::game::Question, ServiceError> {
    let index = game
        .current_question_index
        .ok_or_else(|| ServiceError::InvalidState("no active question: sequence is over".into()))?;
    let expected_question_id = *game
        .question_order
        .get(index)
        .ok_or_else(|| ServiceError::InvalidState("question index out of bounds".into()))?;
    if expected_question_id != question_id {
        return Err(ServiceError::InvalidInput(
            "question id does not match the current question".into(),
        ));
    }

    game.questions_sequence
        .questions
        .get(&question_id)
        .ok_or_else(|| ServiceError::InvalidState("question not found".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dao::{
            game_store::GameStore,
            models::{GameEntity, GameListItemEntity, QuestionsSequenceEntity, TeamEntity},
            storage::StorageResult,
        },
        dto::admin::QuestionValidation,
        state::{
            AppState,
            game::{BlindTestAnswer, BlindTestQuestion, Question, QuestionsSequence},
            state_machine::PauseKind,
        },
    };
    use futures::future::BoxFuture;
    use indexmap::IndexMap;
    use std::{collections::HashMap, sync::Arc};

    #[derive(Clone)]
    struct NoopStore;

    impl GameStore for NoopStore {
        fn save_game(&self, _game: GameEntity) -> BoxFuture<'static, StorageResult<()>> {
            Box::pin(async { Ok(()) })
        }

        fn save_game_without_teams(
            &self,
            _game: GameEntity,
        ) -> BoxFuture<'static, StorageResult<()>> {
            Box::pin(async { Ok(()) })
        }

        fn save_questions_sequence(
            &self,
            _sequence: QuestionsSequenceEntity,
        ) -> BoxFuture<'static, StorageResult<()>> {
            Box::pin(async { Ok(()) })
        }

        fn find_game(&self, _id: Uuid) -> BoxFuture<'static, StorageResult<Option<GameEntity>>> {
            Box::pin(async { Ok(None) })
        }

        fn find_questions_sequence(
            &self,
            _id: Uuid,
        ) -> BoxFuture<'static, StorageResult<Option<QuestionsSequenceEntity>>> {
            Box::pin(async { Ok(None) })
        }

        fn list_games(&self) -> BoxFuture<'static, StorageResult<Vec<GameListItemEntity>>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn list_questions_sequences(
            &self,
        ) -> BoxFuture<'static, StorageResult<Vec<(Uuid, String)>>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn delete_game(&self, _id: Uuid) -> BoxFuture<'static, StorageResult<bool>> {
            Box::pin(async { Ok(false) })
        }

        fn save_team(
            &self,
            _game_id: Uuid,
            _team: TeamEntity,
        ) -> BoxFuture<'static, StorageResult<()>> {
            Box::pin(async { Ok(()) })
        }

        fn delete_team(
            &self,
            _game_id: Uuid,
            _team_id: Uuid,
        ) -> BoxFuture<'static, StorageResult<()>> {
            Box::pin(async { Ok(()) })
        }

        fn health_check(&self) -> BoxFuture<'static, StorageResult<()>> {
            Box::pin(async { Ok(()) })
        }

        fn try_reconnect(&self) -> BoxFuture<'static, StorageResult<()>> {
            Box::pin(async { Ok(()) })
        }
    }

    async fn running_state() -> SharedState {
        let state = AppState::new();
        state.set_game_store(Arc::new(NoopStore)).await;

        let mut answers = HashMap::new();
        answers.insert(
            0,
            BlindTestAnswer {
                key: "title".to_string(),
                value: "Song title".to_string(),
                points: 2,
                is_bonus: false,
            },
        );

        let mut questions = IndexMap::new();
        questions.insert(
            0,
            Question::BlindTest(BlindTestQuestion {
                starts_at_ms: 0,
                guess_duration_ms: 10_000,
                url: "https://example.com/song.mp3".to_string(),
                answers,
            }),
        );

        let sequence = QuestionsSequence::new("Quiz".to_string(), questions);
        let game = GameSession::new("Game".to_string(), IndexMap::new(), sequence, false);
        state
            .with_current_game_slot_mut(|slot| {
                *slot = Some(game);
            })
            .await;

        state
            .run_transition(GameEvent::StartGame, || async { Ok(()) })
            .await
            .unwrap();
        state
            .run_transition(GameEvent::GameConfigured, || async { Ok(()) })
            .await
            .unwrap();

        state
    }

    #[tokio::test]
    async fn marks_answer_found_for_current_question() {
        let state = running_state().await;

        let response = mark_answer_found(
            &state,
            AnswerFoundRequest {
                question_id: 0,
                answer_id: 0,
            },
        )
        .await
        .unwrap();

        assert_eq!(response.answers_ids, vec![0]);
        let answers = state
            .read_current_game(|game| game.unwrap().found_answer_ids.clone())
            .await;
        assert_eq!(answers, vec![0]);
    }

    #[tokio::test]
    async fn reveals_blindtest_answer_as_hint() {
        let state = running_state().await;

        let response = reveal_hint(
            &state,
            QuestionHintRequest {
                question_id: 0,
                hint_id: 0,
            },
        )
        .await
        .unwrap();

        assert_eq!(response.hints_ids, vec![0]);
        let progress = state
            .read_current_game(|game| {
                let game = game.unwrap();
                (
                    game.found_answer_ids.clone(),
                    game.revealed_hint_ids.clone(),
                )
            })
            .await;
        assert_eq!(progress, (Vec::new(), vec![0]));
    }

    #[tokio::test]
    async fn rejects_unknown_answer_and_hint_ids() {
        let state = running_state().await;

        let answer_err = mark_answer_found(
            &state,
            AnswerFoundRequest {
                question_id: 0,
                answer_id: 42,
            },
        )
        .await
        .unwrap_err();
        assert!(answer_err.to_string().contains("does not exist"));

        let hint_err = reveal_hint(
            &state,
            QuestionHintRequest {
                question_id: 0,
                hint_id: 42,
            },
        )
        .await
        .unwrap_err();
        assert!(hint_err.to_string().contains("does not exist"));
    }

    #[tokio::test]
    async fn submits_question_validation_for_current_question_while_paused() {
        let state = running_state().await;
        state
            .run_transition(GameEvent::Pause(PauseKind::Manual), || async { Ok(()) })
            .await
            .unwrap();

        submit_question_validation(
            &state,
            QuestionValidationRequest {
                question_id: 0,
                valid: QuestionValidation::Correct,
            },
        )
        .await
        .unwrap();
    }
}
