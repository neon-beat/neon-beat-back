//! DTO definitions used by the admin REST API and documentation layer.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    dao::models::{GameListItemEntity, PlaylistEntity},
    dto::{
        format_system_time,
        game::{SongSummary, TeamBriefSummary, TeamInput, TeamSummary},
    },
};

/// Minimal projection of a game when listed for administrators.
#[derive(Debug, Serialize, ToSchema)]
pub struct GameListItem {
    pub id: Uuid,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub teams: Vec<TeamBriefSummary>,
    pub playlist: PlaylistListItem,
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
    pub teams: Vec<TeamInput>,
    pub playlist_id: Uuid,
}

#[derive(Deserialize)]
pub struct StartGameQuery {
    #[serde(default)]
    pub shuffle: bool,
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

/// Tri-state result of an answer validation.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnswerValidation {
    Correct,
    Incomplete,
    Wrong,
}

/// Request to validate the current answer submission using a tri-state result.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AnswerValidationRequest {
    pub valid: AnswerValidation,
}

/// Request to adjust a team's score by a delta.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ScoreAdjustmentRequest {
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
    pub team_id: Uuid,
    pub score: i32,
}

#[derive(Debug, Deserialize, ToSchema)]
/// Request payload to create a new team during the prep phase.
#[serde(transparent)]
pub struct CreateTeamRequest(pub TeamInput);

#[derive(Debug, Deserialize, ToSchema)]
/// Request payload to update an existing team in the active game.
#[serde(transparent)]
pub struct UpdateTeamRequest(pub TeamInput);

#[derive(Debug, Deserialize, ToSchema)]
/// Request payload to start a buzzer pairing session.
pub struct StartPairingRequest {
    pub first_team_id: Uuid,
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
    pub teams: Vec<TeamSummary>,
}

/// Errors that can occur when converting storage entities into API DTOs.
#[derive(Debug, Error)]
pub enum ConversionError {
    #[error("playlist id mismatch: expected {expected}, found {found}")]
    MismatchedPlaylistId { expected: Uuid, found: Uuid },
}

impl From<ConversionError> for crate::error::ServiceError {
    fn from(err: ConversionError) -> crate::error::ServiceError {
        crate::error::ServiceError::InvalidState(err.to_string())
    }
}

impl TryFrom<(GameListItemEntity, PlaylistEntity)> for GameListItem {
    type Error = ConversionError;

    fn try_from(
        (game_list_item, playlist): (GameListItemEntity, PlaylistEntity),
    ) -> Result<Self, Self::Error> {
        if playlist.id != game_list_item.playlist_id {
            Err(ConversionError::MismatchedPlaylistId {
                expected: game_list_item.playlist_id,
                found: playlist.id,
            })
        } else {
            Ok(Self {
                id: game_list_item.id,
                name: game_list_item.name,
                created_at: format_system_time(game_list_item.created_at),
                updated_at: format_system_time(game_list_item.updated_at),
                teams: game_list_item.teams.into_iter().map(Into::into).collect(),
                playlist: PlaylistListItem {
                    id: playlist.id,
                    name: playlist.name,
                },
            })
        }
    }
}
