use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::dto::{
    common::{GamePhaseSnapshot, QuestionSnapshot},
    game::TeamSummary,
};

/// Response payload listing the teams currently loaded in memory.
#[derive(Debug, Serialize, ToSchema)]
pub struct TeamsResponse {
    /// List of teams in the active game.
    pub teams: Vec<TeamSummary>,
}

/// Response describing the question currently being played and progress made so far.
#[derive(Debug, Serialize, ToSchema)]
pub struct CurrentQuestionResponse {
    /// Details of the current question.
    pub question: QuestionSnapshot,
    /// Answer IDs already found.
    pub answers_ids: Vec<u8>,
    /// Hint IDs already revealed.
    pub hints_ids: Vec<u8>,
}

/// Response exposing the game's global phase as seen by the public.
#[derive(Debug, Serialize, ToSchema)]
#[serde(transparent)]
pub struct GamePhaseResponse(pub GamePhaseSnapshot);

/// Public response describing the state of the pairing workflow.
#[derive(Debug, Serialize, ToSchema)]
pub struct PairingStatusResponse {
    /// Whether pairing is currently active.
    pub is_pairing: bool,
    /// ID of the team currently pairing (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<Uuid>,
}
