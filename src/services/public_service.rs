//! Service helpers that expose read-only public projections of the current game.

use crate::{
    dto::{
        common::QuestionSnapshot,
        game::TeamSummary,
        public::{
            CurrentQuestionResponse, GamePhaseResponse, PairingStatusResponse, TeamsResponse,
        },
    },
    error::ServiceError,
    state::{
        SharedState,
        state_machine::{GamePhase, GameRunningPhase, PrepStatus},
    },
};

/// Return the current teams exposed to the public UI.
pub async fn get_teams(state: &SharedState) -> Result<TeamsResponse, ServiceError> {
    let teams = state
        .with_current_game(|game| {
            Ok(game
                .teams
                .clone()
                .into_iter()
                .map(TeamSummary::from)
                .collect())
        })
        .await?;
    Ok(TeamsResponse { teams })
}

/// Return the question being played alongside any progress already made.
pub async fn get_current_question(
    state: &SharedState,
) -> Result<CurrentQuestionResponse, ServiceError> {
    state
        .with_current_game(|game| {
            let index = game.current_question_index.ok_or_else(|| {
                ServiceError::NotFound("no active question: sequence is over".into())
            })?;
            let (question_id, question) = game.get_question(index).ok_or_else(|| {
                ServiceError::InvalidState("question not found in sequence".into())
            })?;

            let question = QuestionSnapshot::from_game_question(question_id, &question);
            Ok(CurrentQuestionResponse {
                question,
                answers_ids: game.found_answer_ids.clone(),
                hints_ids: game.revealed_hint_ids.clone(),
            })
        })
        .await
}

/// Return the current game phase (e.g. idle, playing, reveal) and degraded mode.
pub async fn get_game_phase(state: &SharedState) -> Result<GamePhaseResponse, ServiceError> {
    let phase = state.state_machine_phase().await;
    let snapshot = state.game_phase_snapshot(&phase).await;
    Ok(GamePhaseResponse(snapshot))
}

/// Return the current pairing workflow status for public consumers.
pub async fn get_pairing_status(
    state: &SharedState,
) -> Result<PairingStatusResponse, ServiceError> {
    match state.state_machine_phase().await {
        GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Ready)) => {
            Ok(PairingStatusResponse {
                is_pairing: false,
                team_id: None,
            })
        }
        GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Pairing(session))) => {
            Ok(PairingStatusResponse {
                is_pairing: true,
                team_id: Some(session.pairing_team_id),
            })
        }
        _ => Ok(PairingStatusResponse {
            is_pairing: false,
            team_id: None,
        }),
    }
}
