//! DTO definitions used by the admin REST API and documentation layer.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::dto::game::{PlayerInput, SongSummary};

/// Minimal projection of a game when listed for administrators.
#[derive(Debug, Serialize, ToSchema)]
pub struct GameListItem {
    pub id: Uuid,
    pub name: String,
}

/// Minimal projection of a playlist available for game creation.
#[derive(Debug, Serialize, ToSchema)]
pub struct PlaylistListItem {
    pub id: Uuid,
    pub name: String,
}

/// Payload describing how to spin up a game from an existing playlist definition.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateGameRequest {
    pub name: String,
    pub players: Vec<PlayerInput>,
    pub playlist_id: Uuid,
}

/// Classifies the type of field discovered during gameplay.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Point,
    Bonus,
}

/// Request to mark a point or bonus field as revealed.
#[derive(Debug, Deserialize, ToSchema)]
pub struct MarkFieldRequest {
    pub song_id: u32,
    pub field_key: String,
    pub kind: FieldKind,
}

/// Response summarising the fields uncovered for the current song.
#[derive(Debug, Serialize, ToSchema)]
pub struct FieldsFoundResponse {
    pub song_id: u32,
    pub point_fields: Vec<String>,
    pub bonus_fields: Vec<String>,
}

/// Request to validate or reject the current answer submission.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AnswerValidationRequest {
    pub valid: bool,
}

/// Request to adjust a team's score by a delta.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ScoreAdjustmentRequest {
    pub buzzer_id: String,
    pub delta: i32,
}

/// Generic action acknowledgement used by admin endpoints.
#[derive(Debug, Serialize, ToSchema)]
pub struct ActionResponse {
    pub message: String,
}

/// Result of a score adjustment, returning the updated tally.
#[derive(Debug, Serialize, ToSchema)]
pub struct ScoreUpdateResponse {
    pub buzzer_id: String,
    pub score: i32,
}

/// Response emitted when a game starts, including the initial song details.
#[derive(Debug, Serialize, ToSchema)]
pub struct StartGameResponse {
    pub song: SongSummary,
}

/// Response describing the state of the playlist after moving to the next song.
#[derive(Debug, Serialize, ToSchema)]
pub struct NextSongResponse {
    pub finished: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub song: Option<SongSummary>,
}

/// Response returned when a game is stopped, gathering final team scores.
#[derive(Debug, Serialize, ToSchema)]
pub struct StopGameResponse {
    pub teams: Vec<crate::dto::sse::TeamSummary>,
}
