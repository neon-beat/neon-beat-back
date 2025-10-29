//! DTO definitions used by the admin REST API and documentation layer.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::{Validate, ValidationErrors};

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
    /// Minimal playlist information.
    pub playlist: PlaylistListItem,
}

/// Minimal projection of a playlist available for game creation.
#[derive(Debug, Serialize, ToSchema)]
pub struct PlaylistListItem {
    /// Unique identifier for the playlist.
    pub id: Uuid,
    /// Display name of the playlist.
    pub name: String,
}

/// Payload describing how to spin up a game from an existing playlist definition.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct CreateGameRequest {
    /// Display name for the new game.
    pub name: String,
    /// List of teams participating in the game.
    #[validate(nested)]
    pub teams: Vec<TeamInput>,
    /// ID of the playlist to use for this game.
    pub playlist_id: Uuid,
}

/// Query parameters for game creation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateGameQuery {
    /// Whether to shuffle the playlist order.
    #[serde(default)]
    pub shuffle: bool,
}

/// Query parameters for loading an existing game.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoadGameQuery {
    /// Whether to shuffle the playlist order.
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

/// Classifies the type of field discovered during gameplay.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    /// A regular point field.
    Point,
    /// A bonus field.
    Bonus,
}

/// Request to mark a point or bonus field as revealed.
#[derive(Debug, Deserialize, ToSchema)]
pub struct MarkFieldRequest {
    /// ID of the song containing the field.
    pub song_id: u32,
    /// Key identifying the field within the song.
    pub field_key: String,
    /// Type of field being marked.
    pub kind: FieldKind,
}

/// Response summarising the fields uncovered for the current song.
#[derive(Debug, Serialize, ToSchema)]
pub struct FieldsFoundResponse {
    /// ID of the current song.
    pub song_id: u32,
    /// List of point field keys that have been found.
    pub point_fields: Vec<String>,
    /// List of bonus field keys that have been found.
    pub bonus_fields: Vec<String>,
}

/// Tri-state result of an answer validation.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnswerValidation {
    /// Answer is completely correct.
    Correct,
    /// Answer is partially correct but incomplete.
    Incomplete,
    /// Answer is incorrect.
    Wrong,
}

/// Request to validate the current answer submission using a tri-state result.
#[derive(Debug, Deserialize, ToSchema)]
pub struct AnswerValidationRequest {
    /// Validation result for the answer.
    pub valid: AnswerValidation,
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

/// Response emitted when a game starts, including the initial song details.
#[derive(Debug, Serialize, ToSchema)]
pub struct StartGameResponse {
    /// Summary of the first song in the game.
    pub song: SongSummary,
}

/// Response describing the state of the playlist after moving to the next song.
#[derive(Debug, Serialize, ToSchema)]
pub struct NextSongResponse {
    /// Whether the playlist has been completed.
    pub finished: bool,
    /// Summary of the next song, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub song: Option<SongSummary>,
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
    /// Playlist ID in game entity does not match the provided playlist.
    #[error("playlist id mismatch: expected {expected}, found {found}")]
    MismatchedPlaylistId {
        /// Expected playlist ID from the game.
        expected: Uuid,
        /// Actual playlist ID found.
        found: Uuid,
    },
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
