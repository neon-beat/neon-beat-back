use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::dto::game::{PlayerInput, SongSummary};

#[derive(Debug, Serialize, ToSchema)]
pub struct GameListItem {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlaylistListItem {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateGameFromPlaylistRequest {
    pub name: String,
    pub players: Vec<PlayerInput>,
    pub playlist_id: String,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Point,
    Bonus,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct MarkFieldRequest {
    pub field_key: String,
    pub kind: FieldKind,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FieldsFoundResponse {
    pub song_id: String,
    pub point_fields: Vec<String>,
    pub bonus_fields: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AnswerValidationRequest {
    pub valid: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ScoreAdjustmentRequest {
    pub buzzer_id: String,
    pub delta: i32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ActionResponse {
    pub message: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ScoreUpdateResponse {
    pub buzzer_id: String,
    pub score: i32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StartGameResponse {
    pub song: SongSummary,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct NextSongResponse {
    pub finished: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub song: Option<SongSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct StopGameResponse {
    pub teams: Vec<crate::dto::sse::TeamSummary>,
}
