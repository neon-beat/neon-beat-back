use serde::Serialize;
use utoipa::ToSchema;

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
    /// Whether the backend is running without a MongoDB connection.
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
/// Payload carrying the list of teams when a game starts or is loaded.
pub struct TeamsEvent {
    pub teams: Vec<TeamSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TeamSummary {
    pub buzzer_id: String,
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
    pub valid: bool,
}

#[derive(Debug, Serialize, ToSchema)]
/// Broadcast whenever the gameplay phase changes.
pub struct PhaseChangedEvent {
    pub phase: PhaseSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub song: Option<SongSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scoreboard: Option<Vec<TeamSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused_buzzer: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PhaseSnapshot {
    pub kind: String,
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

impl From<crate::state::game::Player> for TeamSummary {
    fn from(player: crate::state::game::Player) -> Self {
        Self {
            buzzer_id: player.buzzer_id,
            name: player.name,
            score: player.score,
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
