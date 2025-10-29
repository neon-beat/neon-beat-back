use std::collections::HashSet;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::{Validate, ValidationErrors};

use crate::{
    dto::{common::TeamColorDto, format_system_time, validation::validate_buzzer_id},
    state::game::{GameSession, Playlist, PointField, Song, Team},
};

/// Payload used to bootstrap a brand-new game instance.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct CreateGameWithPlaylistRequest {
    /// Display name for the new game.
    pub name: String,
    /// List of teams participating in the game.
    #[validate(nested)]
    pub teams: Vec<TeamInput>,
    /// Playlist definition for the game.
    #[validate(nested)]
    pub playlist: PlaylistInput,
}

/// Incoming team definition for the game bootstrap.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TeamInput {
    /// Display name for the team.
    pub name: String,
    /// If not specified, does not change it (or lets the back use the default value).
    /// If null is specified, removes the buzzer ID.
    /// If a string is specified, sets the buzzer ID to this string.
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub buzzer_id: Option<Option<String>>,
    /// Initial score for the team (defaults to 0 if omitted).
    #[serde(default)]
    #[schema(value_type = i32)]
    pub score: Option<i32>,
    /// Optional HSV color. If omitted, the backend chooses the first unused color from the
    /// configured colors set.
    #[serde(default)]
    #[schema(value_type = TeamColorDto)]
    pub color: Option<TeamColorDto>,
}

impl Validate for TeamInput {
    fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();

        // Validate buzzer_id if present
        if let Some(Some(ref id)) = self.buzzer_id {
            if let Err(e) = validate_buzzer_id(id) {
                errors.add("buzzer_id", e);
            }
        }

        // Validate color if present
        if let Some(ref color) = self.color {
            if let Err(color_errors) = color.validate() {
                errors.merge_self("color", Err(color_errors));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Playlist metadata and songs supplied when bootstrapping a game.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct PlaylistInput {
    /// Display name for the playlist.
    pub name: String,
    /// List of songs in the playlist.
    #[validate(nested)]
    pub songs: Vec<SongInput>,
}

/// Song details required to populate a playlist.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct SongInput {
    /// Start time in milliseconds for the song playback.
    pub starts_at_ms: usize,
    /// Duration in milliseconds for guessing.
    pub guess_duration_ms: usize,
    /// URL of the song media file.
    #[validate(url)]
    pub url: String,
    /// Point fields (required information) for this song.
    pub point_fields: Vec<PointFieldInput>,
    /// Bonus fields (optional extra information) for this song.
    #[serde(default)]
    pub bonus_fields: Vec<PointFieldInput>,
}

/// Point field details required for a song.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PointFieldInput {
    /// Unique key identifying this field.
    pub key: String,
    /// The answer/value for this field.
    pub value: String,
    /// Points awarded for finding this field.
    pub points: u8,
}

/// Summary returned once a game has been created or loaded.
#[derive(Debug, Serialize, ToSchema)]
pub struct GameSummary {
    /// Unique identifier for the game.
    pub id: String,
    /// Display name of the game.
    pub name: String,
    /// RFC3339 timestamp when the game was created.
    pub created_at: String,
    /// RFC3339 timestamp when the game was last updated.
    pub updated_at: String,
    /// List of teams in the game.
    pub teams: Vec<TeamSummary>,
    /// Summary of the playlist used in the game.
    pub playlist: PlaylistSummary,
    /// Index of the current song being played (if any).
    pub current_song_index: Option<usize>,
}

/// Public projection of a team exposed to REST/SSE clients.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct TeamSummary {
    /// Unique identifier for the team.
    pub id: Uuid,
    /// ID of the buzzer assigned to this team.
    pub buzzer_id: Option<String>,
    /// Display name of the team.
    pub name: String,
    /// Current score for the team.
    pub score: i32,
    /// HSV color assigned to the team.
    pub color: TeamColorDto,
}

/// Brief team information without score or color.
#[derive(Debug, Serialize, ToSchema)]
pub struct TeamBriefSummary {
    /// Unique identifier for the team.
    pub id: Uuid,
    /// Display name of the team.
    pub name: String,
}

/// Summary of a playlist including all its songs.
#[derive(Debug, Serialize, ToSchema)]
pub struct PlaylistSummary {
    /// Unique identifier for the playlist.
    pub id: Uuid,
    /// Display name of the playlist.
    pub name: String,
    /// List of songs in the playlist.
    pub songs: Vec<SongSummary>,
}

/// Summary of a single song within a playlist.
#[derive(Debug, Serialize, ToSchema)]
pub struct SongSummary {
    /// Unique identifier for the song.
    pub id: String,
    /// Start time in milliseconds for playback.
    pub starts_at_ms: usize,
    /// Duration in milliseconds for guessing.
    pub guess_duration_ms: usize,
    /// URL of the song media file.
    pub url: String,
    /// Required point fields for this song.
    pub point_fields: Vec<PointFieldSummary>,
    /// Optional bonus fields for this song.
    pub bonus_fields: Vec<PointFieldSummary>,
}

/// Summary of a point or bonus field within a song.
#[derive(Debug, Serialize, ToSchema)]
pub struct PointFieldSummary {
    /// Unique key identifying this field.
    pub key: String,
    /// The answer/value for this field.
    pub value: String,
    /// Points awarded for finding this field.
    pub points: u8,
}

/// Errors that can occur when validating playlist song ordering.
#[derive(Debug, Error)]
pub enum PlaylistOrderError {
    /// Song IDs in the order don't match the playlist songs.
    #[error("playlist ids mismatch (missing in order: {missing:?}, extra in order: {extra:?})")]
    MismatchedIds {
        /// Song IDs present in playlist but missing from order.
        missing: Vec<u32>,
        /// Song IDs present in order but not in playlist.
        extra: Vec<u32>,
    },
}

impl From<PointField> for PointFieldSummary {
    fn from(field: PointField) -> Self {
        Self {
            key: field.key,
            value: field.value,
            points: field.points,
        }
    }
}

impl From<(Uuid, Team)> for TeamSummary {
    fn from((id, team): (Uuid, Team)) -> Self {
        Self {
            id,
            buzzer_id: team.buzzer_id,
            name: team.name,
            score: team.score,
            color: team.color.into(),
        }
    }
}

impl From<(u32, Song)> for SongSummary {
    fn from((id, song): (u32, Song)) -> Self {
        Self {
            id: id.to_string(),
            starts_at_ms: song.starts_at_ms,
            guess_duration_ms: song.guess_duration_ms,
            url: song.url,
            point_fields: song.point_fields.into_iter().map(Into::into).collect(),
            bonus_fields: song.bonus_fields.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<(Playlist, Vec<u32>)> for PlaylistSummary {
    fn from((playlist, order): (Playlist, Vec<u32>)) -> Self {
        let songs = ordered_song_summaries(playlist.songs, order).unwrap_or_else(|e| {
            panic!(
                "Error when generating PlaylistSummary (should not happen because should be checked before) : {}",
                e
            )
        });
        Self {
            id: playlist.id,
            name: playlist.name,
            songs,
        }
    }
}

impl From<GameSession> for GameSummary {
    fn from(session: GameSession) -> Self {
        let playlist_summary = (session.playlist, session.playlist_song_order).into();

        Self {
            id: session.id.to_string(),
            name: session.name,
            created_at: format_system_time(session.created_at),
            updated_at: format_system_time(session.updated_at),
            teams: session.teams.into_iter().map(Into::into).collect(),
            playlist: playlist_summary,
            current_song_index: session.current_song_index,
        }
    }
}

fn ordered_song_summaries(
    playlist_songs: IndexMap<u32, Song>,
    order: Vec<u32>,
) -> Result<Vec<SongSummary>, PlaylistOrderError> {
    let playlist_ids = playlist_songs.keys().cloned().collect::<HashSet<_>>();
    let order_ids = order.iter().copied().collect::<HashSet<_>>();

    if playlist_ids != order_ids {
        let mut missing = playlist_ids
            .difference(&order_ids)
            .copied()
            .collect::<Vec<_>>();
        let mut extra = order_ids
            .difference(&playlist_ids)
            .copied()
            .collect::<Vec<_>>();

        missing.sort_unstable();
        extra.sort_unstable();

        return Err(PlaylistOrderError::MismatchedIds { missing, extra });
    }

    order
        .into_iter()
        .map(|song_id| {
            let Some(song_ref) = playlist_songs.get(&song_id) else {
                // Safety: mismatch should have been caught above, but guard defensively.
                return Err(PlaylistOrderError::MismatchedIds {
                    missing: vec![song_id],
                    extra: Vec::new(),
                });
            };

            Ok((song_id, song_ref.clone()).into())
        })
        .collect::<Result<Vec<SongSummary>, _>>()
}
