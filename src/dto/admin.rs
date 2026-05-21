//! DTO definitions used by the admin REST API and documentation layer.
#![allow(deprecated)]

use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::{Validate, ValidationErrors};

use crate::{
    dao::models::{GameListItemEntity, QuestionsSequenceEntity},
    dto::{
        format_system_time,
        game::{QuestionSummary, TeamBriefSummary, TeamInput, TeamSummary},
    },
};

/// Minimal projection of a game when listed for administrators.
#[derive(Debug, Serialize, ToSchema)]
pub struct GameListItem {
    /// Unique identifier for the game.
    pub id: Uuid,
    /// Display name of the game.
    pub name: String,
    /// RFC3339 timestamp when the game was created.
    pub created_at: String,
    /// RFC3339 timestamp when the game was last updated.
    pub updated_at: String,
    /// Brief summaries of teams in the game.
    pub teams: Vec<TeamBriefSummary>,
    /// Minimal questions sequence information.
    pub questions_sequence: QuestionsSequenceListItem,
}

/// Minimal projection of a questions sequence available for game creation.
#[derive(Debug, Serialize, ToSchema)]
pub struct QuestionsSequenceListItem {
    /// Unique identifier for the questions sequence.
    pub id: Uuid,
    /// Display name of the questions sequence.
    pub name: String,
}

/// Deprecated playlist projection returned by legacy playlist routes.
#[deprecated(
    since = "0.9.0",
    note = "Deprecated legacy playlist compatibility. Use QuestionsSequenceListItem and /admin/questions-sequence instead."
)]
#[derive(Debug, Serialize, ToSchema)]
pub struct LegacyPlaylistListItem {
    /// Unique identifier for the playlist.
    pub id: String,
    /// Display name of the playlist.
    pub name: String,
}

impl From<QuestionsSequenceListItem> for LegacyPlaylistListItem {
    fn from(value: QuestionsSequenceListItem) -> Self {
        Self {
            id: value.id.to_string(),
            name: value.name,
        }
    }
}

/// Payload describing how to spin up a game from an existing questions sequence.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct CreateGameRequest {
    /// Display name for the new game.
    pub name: String,
    /// List of teams participating in the game.
    #[validate(nested)]
    pub teams: Vec<TeamInput>,
    /// ID of the questions sequence to use for this game.
    pub questions_sequence_id: Uuid,
}

/// Query parameters for game creation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateGameQuery {
    /// Whether to shuffle the question order.
    #[serde(default)]
    pub shuffle: bool,
}

/// Query parameters for loading an existing game.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadGameQuery {
    /// Whether to shuffle the question order.
    #[serde(default)]
    pub shuffle: bool,
}

/// Rejects any query parameters by failing deserialization on unknown fields.
///
/// Used for routes that should not accept any query parameters. When a client
/// provides any query parameter to a route using this type, Axum will return
/// a `400 Bad Request` with a descriptive serde error message.
///
/// # Example
///
/// ```rust,ignore
/// pub async fn my_handler(
///     Query(_no_query): Query<NoQuery>,
/// ) -> Result<Json<Response>, ServiceError> {
///     // This route rejects any query parameters
///     Ok(Json(response))
/// }
/// ```
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct NoQuery {}

/// Request to mark an answer as found.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AnswerFoundRequest {
    /// ID of the question containing the answer.
    pub question_id: u32,
    /// ID identifying the answer within the question.
    pub answer_id: u8,
}

/// Response summarising the answers found for the current question.
#[derive(Debug, Serialize, ToSchema)]
pub struct AnswersFoundResponse {
    /// ID of the current question.
    pub question_id: u32,
    /// List of answer IDs that have been found.
    pub answers_ids: Vec<u8>,
}

/// Request to reveal a hint for a question.
#[derive(Debug, Deserialize, ToSchema)]
pub struct QuestionHintRequest {
    /// ID of the question containing the hint.
    pub question_id: u32,
    /// ID identifying the hint within the question.
    pub hint_id: u8,
}

/// Response summarising the hints revealed for the current question.
#[derive(Debug, Serialize, ToSchema)]
pub struct QuestionHintsResponse {
    /// ID of the current question.
    pub question_id: u32,
    /// List of hint IDs that have been revealed.
    pub hints_ids: Vec<u8>,
}

/// Tri-state result of a question validation.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuestionValidation {
    /// Answer is completely correct.
    Correct,
    /// Answer is partially correct but incomplete.
    Incomplete,
    /// Answer is incorrect.
    Wrong,
}

/// Request to submit the current question validation using a tri-state result.
#[derive(Debug, Deserialize, ToSchema)]
pub struct QuestionValidationRequest {
    /// ID of the question being validated.
    pub question_id: u32,
    /// Validation result for the question answer submission.
    pub valid: QuestionValidation,
}

/// Request to adjust a team's score by a delta.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ScoreAdjustmentRequest {
    /// Points to add (positive) or subtract (negative).
    pub delta: i32,
}

/// Generic action acknowledgement used by admin endpoints.
#[derive(Debug, Serialize, ToSchema)]
pub struct ActionResponse {
    /// Human-readable message describing the action result.
    pub message: String,
}

/// Result of a score adjustment, returning the updated tally.
#[derive(Debug, Serialize, ToSchema)]
pub struct ScoreUpdateResponse {
    /// ID of the team whose score was updated.
    pub team_id: Uuid,
    /// New score after adjustment.
    pub score: i32,
}

#[derive(Debug, Deserialize, ToSchema)]
/// Request payload to create a new team during the prep phase.
#[serde(transparent)]
pub struct CreateTeamRequest(pub TeamInput);

impl Validate for CreateTeamRequest {
    fn validate(&self) -> Result<(), ValidationErrors> {
        self.0.validate()
    }
}

#[derive(Debug, Deserialize, ToSchema)]
/// Request payload to update an existing team in the active game.
#[serde(transparent)]
pub struct UpdateTeamRequest(pub TeamInput);

impl Validate for UpdateTeamRequest {
    fn validate(&self) -> Result<(), ValidationErrors> {
        self.0.validate()
    }
}

/// Request payload to start a buzzer pairing session.
#[derive(Debug, Deserialize, ToSchema)]
pub struct StartPairingRequest {
    /// ID of the first team to pair their buzzer.
    pub first_team_id: Uuid,
}

/// Response emitted when a game starts, including the initial question details.
#[derive(Debug, Serialize, ToSchema)]
pub struct StartGameResponse {
    /// Summary of the first question in the game.
    pub question: QuestionSummary,
}

/// Response describing the state of the sequence after moving to the next question.
#[derive(Debug, Serialize, ToSchema)]
pub struct NextQuestionResponse {
    /// Whether the sequence has been completed.
    pub finished: bool,
    /// Summary of the next question, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<QuestionSummary>,
}

/// Response returned when a game is stopped, gathering final team scores.
#[derive(Debug, Serialize, ToSchema)]
pub struct StopGameResponse {
    /// Final scores and details for all teams.
    pub teams: Vec<TeamSummary>,
}

/// Errors that can occur when converting storage entities into API DTOs.
#[derive(Debug, Error)]
pub enum ConversionError {
    /// Questions sequence ID in game entity does not match the provided sequence.
    #[error("questions sequence id mismatch: expected {expected}, found {found}")]
    MismatchedQuestionsSequenceId {
        /// Expected questions sequence ID from the game.
        expected: Uuid,
        /// Actual questions sequence ID found.
        found: Uuid,
    },
}

impl From<ConversionError> for crate::error::ServiceError {
    fn from(err: ConversionError) -> crate::error::ServiceError {
        crate::error::ServiceError::InvalidState(err.to_string())
    }
}

impl TryFrom<(GameListItemEntity, QuestionsSequenceEntity)> for GameListItem {
    type Error = ConversionError;

    fn try_from(
        (game_list_item, questions_sequence): (GameListItemEntity, QuestionsSequenceEntity),
    ) -> Result<Self, Self::Error> {
        if questions_sequence.id != game_list_item.questions_sequence_id {
            Err(ConversionError::MismatchedQuestionsSequenceId {
                expected: game_list_item.questions_sequence_id,
                found: questions_sequence.id,
            })
        } else {
            Ok(Self {
                id: game_list_item.id,
                name: game_list_item.name,
                created_at: format_system_time(game_list_item.created_at),
                updated_at: format_system_time(game_list_item.updated_at),
                teams: game_list_item.teams.into_iter().map(Into::into).collect(),
                questions_sequence: QuestionsSequenceListItem {
                    id: questions_sequence.id,
                    name: questions_sequence.name,
                },
            })
        }
    }
}
