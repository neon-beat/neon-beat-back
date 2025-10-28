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
    pub name: String,
    #[validate(nested)]
    pub teams: Vec<TeamInput>,
    #[validate(nested)]
    pub playlist: PlaylistInput,
}

/// Incoming team definition for the game bootstrap.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TeamInput {
    pub name: String,
    /// If not specified, does not change it (or lets the back use the default value).
    /// If null is specified, removes the buzzer ID.
    /// If a string is specified, sets the buzzer ID to this string.
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub buzzer_id: Option<Option<String>>,
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
    pub name: String,
    #[validate(nested)]
    pub songs: Vec<SongInput>,
}

/// Song details required to populate a playlist.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct SongInput {
    pub starts_at_ms: usize,
    pub guess_duration_ms: usize,
    #[validate(url)]
    pub url: String,
    pub point_fields: Vec<PointFieldInput>,
    #[serde(default)]
    pub bonus_fields: Vec<PointFieldInput>,
}

/// Point field details required for a song.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PointFieldInput {
    pub key: String,
    pub value: String,
    pub points: u8,
}

/// Summary returned once a game has been created or loaded.
#[derive(Debug, Serialize, ToSchema)]
pub struct GameSummary {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub teams: Vec<TeamSummary>,
    pub playlist: PlaylistSummary,
    pub current_song_index: Option<usize>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
/// Public projection of a team exposed to REST/SSE clients.
pub struct TeamSummary {
    pub id: Uuid,
    pub buzzer_id: Option<String>,
    pub name: String,
    pub score: i32,
    pub color: TeamColorDto,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TeamBriefSummary {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlaylistSummary {
    pub id: Uuid,
    pub name: String,
    pub songs: Vec<SongSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SongSummary {
    pub id: String,
    pub starts_at_ms: usize,
    pub guess_duration_ms: usize,
    pub url: String,
    pub point_fields: Vec<PointFieldSummary>,
    pub bonus_fields: Vec<PointFieldSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PointFieldSummary {
    pub key: String,
    pub value: String,
    pub points: u8,
}

#[derive(Debug, Error)]
pub enum PlaylistOrderError {
    #[error("playlist ids mismatch (missing in order: {missing:?}, extra in order: {extra:?})")]
    MismatchedIds { missing: Vec<u32>, extra: Vec<u32> },
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
