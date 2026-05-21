use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::dto::{admin::QuestionValidation, common::GamePhaseSnapshot, game::TeamSummary};

/// Dispatched payload carried across SSE channels.
#[derive(Clone, Debug)]
pub struct ServerEvent {
    /// Optional event type name for the SSE message.
    pub event: Option<String>,
    /// The serialized JSON data for the event.
    pub data: String,
}

impl ServerEvent {
    /// Convenience wrapper that serialises `payload` into the SSE data field.
    pub fn json<E, T>(event: E, payload: &T) -> serde_json::Result<Self>
    where
        E: Into<Option<String>>,
        T: Serialize,
    {
        Ok(Self {
            event: event.into(),
            data: serde_json::to_string(payload)?,
        })
    }
}

/// Initial metadata sent to an SSE client when it connects.
#[derive(Debug, Serialize, ToSchema)]
pub struct Handshake {
    /// Identifier of the SSE stream (`public` or `admin`).
    pub stream: String,
    /// Human-readable message confirming the subscription.
    pub message: String,
    /// Whether the backend is running without a storage backend connection.
    pub degraded: bool,
    /// Optional admin token returned when the stream is privileged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// Broadcast when the backend enters or leaves degraded mode.
#[derive(Debug, Serialize, ToSchema)]
pub struct SystemStatus {
    /// Whether the system is in degraded mode.
    pub degraded: bool,
}

/// Broadcast when answers have been marked as found.
#[derive(Debug, Serialize, ToSchema)]
pub struct QuestionFoundAnswersEvent {
    /// ID of the question containing the answers.
    pub question_id: u32,
    /// IDs of answers that have been found.
    pub answers_ids: Vec<u8>,
}

/// Broadcast when question hints have been revealed.
#[derive(Debug, Serialize, ToSchema)]
pub struct QuestionHintsEvent {
    /// ID of the question containing the hints.
    pub question_id: u32,
    /// IDs of hints that have been revealed.
    pub hints_ids: Vec<u8>,
}

/// Broadcast when an answer has been validated or invalidated.
#[derive(Debug, Serialize, ToSchema)]
pub struct AnswerValidationEvent {
    /// ID of the question being validated.
    pub question_id: u32,
    /// Validation result for the answer.
    pub valid: QuestionValidation,
}

/// Broadcast whenever the gameplay phase changes.
#[derive(Debug, Serialize, ToSchema)]
#[serde(transparent)]
pub struct PhaseChangedEvent(pub GamePhaseSnapshot);

/// Event emitted when the pairing workflow awaits the next team.
#[derive(Debug, Serialize, ToSchema)]
pub struct PairingWaitingEvent {
    /// ID of the team that should pair their buzzer.
    pub team_id: Uuid,
}

/// Event emitted when a buzzer has been assigned during pairing.
#[derive(Debug, Serialize, ToSchema)]
pub struct PairingAssignedEvent {
    /// ID of the team that was paired.
    pub team_id: Uuid,
    /// ID of the buzzer that was assigned.
    pub buzzer_id: String,
}

/// Event emitted when pairing is aborted and teams restored.
#[derive(Debug, Serialize, ToSchema)]
pub struct PairingRestoredEvent {
    /// Snapshot of teams restored to their pre-pairing state.
    pub snapshot: Vec<TeamSummary>,
}

/// Event emitted when a buzzer buzzes during prep ready mode.
#[derive(Debug, Serialize, ToSchema)]
pub struct TestBuzzEvent {
    /// ID of the team whose buzzer was pressed.
    pub team_id: Uuid,
}

/// Event emitted when a new team is created.
#[derive(Debug, Serialize, ToSchema)]
pub struct TeamCreatedEvent {
    /// The newly created team.
    pub team: TeamSummary,
}

/// Event emitted when a team has been deleted.
#[derive(Debug, Serialize, ToSchema)]
pub struct TeamDeletedEvent {
    /// ID of the team that was deleted.
    pub team_id: Uuid,
}

/// Event emitted when an existing team was updated (name, buzzer, or score).
#[derive(Debug, Serialize, ToSchema)]
pub struct TeamUpdatedEvent {
    /// The updated team with new information.
    pub team: TeamSummary,
}
