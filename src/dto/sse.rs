use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::dto::{admin::AnswerValidation, common::GamePhaseSnapshot, game::TeamSummary};

#[derive(Clone, Debug)]
/// Dispatched payload carried across SSE channels.
pub struct ServerEvent {
    pub event: Option<String>,
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

#[derive(Debug, Serialize, ToSchema)]
/// Initial metadata sent to an SSE client when it connects.
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

#[derive(Debug, Serialize, ToSchema)]
/// Broadcast when the backend enters or leaves degraded mode.
pub struct SystemStatus {
    pub degraded: bool,
}

#[derive(Debug, Serialize, ToSchema)]
/// Broadcast when point or bonus fields have been marked as found.
pub struct FieldsFoundEvent {
    pub song_id: u32,
    pub point_fields: Vec<String>,
    pub bonus_fields: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
/// Broadcast when an answer has been validated or invalidated.
pub struct AnswerValidationEvent {
    pub valid: AnswerValidation,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(transparent)]
/// Broadcast whenever the gameplay phase changes.
pub struct PhaseChangedEvent(pub GamePhaseSnapshot);

#[derive(Debug, Serialize, ToSchema)]
/// Event emitted when the pairing workflow awaits the next team.
pub struct PairingWaitingEvent {
    pub team_id: Uuid,
}

#[derive(Debug, Serialize, ToSchema)]
/// Event emitted when a buzzer has been assigned during pairing.
pub struct PairingAssignedEvent {
    pub team_id: Uuid,
    pub buzzer_id: String,
}

#[derive(Debug, Serialize, ToSchema)]
/// Event emitted when pairing is aborted and teams restored.
pub struct PairingRestoredEvent {
    pub snapshot: Vec<TeamSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
/// Event emitted when a buzzer buzzes during prep ready mode.
pub struct TestBuzzEvent {
    pub team_id: Uuid,
}

#[derive(Debug, Serialize, ToSchema)]
/// Event emitted when a new team is created.
pub struct TeamCreatedEvent {
    pub team: TeamSummary,
}

#[derive(Debug, Serialize, ToSchema)]
/// Event emitted when a team has been deleted.
pub struct TeamDeletedEvent {
    pub team_id: Uuid,
}

#[derive(Debug, Serialize, ToSchema)]
/// Event emitted when an existing team was updated (name, buzzer, or score).
pub struct TeamUpdatedEvent {
    pub team: TeamSummary,
}
