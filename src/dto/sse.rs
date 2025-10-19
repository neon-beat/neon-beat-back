use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::dto::{admin::AnswerValidation, phase::VisibleGamePhase};

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

#[derive(Debug, Serialize, ToSchema, Clone)]
/// Summary of a team broadcast to SSE subscribers.
pub struct TeamSummary {
    pub id: Uuid,
    pub buzzer_id: Option<String>,
    pub name: String,
    pub score: i32,
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
/// Broadcast whenever the gameplay phase changes.
pub struct PhaseChangedEvent {
    pub phase: VisibleGamePhase,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub song: Option<SongSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scoreboard: Option<Vec<TeamSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused_buzzer: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SongSnapshot {
    pub id: u32,
    pub starts_at_ms: usize,
    pub guess_duration_ms: usize,
    pub url: String,
    pub point_fields: Vec<PointFieldSnapshot>,
    pub bonus_fields: Vec<PointFieldSnapshot>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PointFieldSnapshot {
    pub key: String,
    pub value: String,
    pub points: u8,
}

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

impl From<crate::state::game::Team> for TeamSummary {
    fn from(team: crate::state::game::Team) -> Self {
        Self {
            id: team.id,
            buzzer_id: team.buzzer_id,
            name: team.name,
            score: team.score,
        }
    }
}

impl From<crate::state::game::PointField> for PointFieldSnapshot {
    fn from(field: crate::state::game::PointField) -> Self {
        Self {
            key: field.key,
            value: field.value,
            points: field.points,
        }
    }
}
