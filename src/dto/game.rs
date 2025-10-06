use std::collections::HashSet;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;

use crate::state::game::{GameSession, Player, Playlist, PointField, Song};

/// Payload used to bootstrap a brand-new game instance.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateGameWithPlaylistRequest {
    pub name: String,
    pub players: Vec<PlayerInput>,
    pub playlist: PlaylistInput,
}

/// Incoming player definition for the game bootstrap.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PlayerInput {
    pub buzzer_id: String,
    pub name: String,
}

/// Playlist metadata and songs supplied when bootstrapping a game.
#[derive(Debug, Deserialize, ToSchema)]
pub struct PlaylistInput {
    pub name: String,
    pub songs: Vec<SongInput>,
}

/// Song details required to populate a playlist.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SongInput {
    pub starts_at_ms: u64,
    pub guess_duration_ms: u64,
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
    pub points: i8,
}

/// Summary returned once a game has been created or loaded.
#[derive(Debug, Serialize, ToSchema)]
pub struct GameSummary {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub players: Vec<PlayerSummary>,
    pub playlist: PlaylistSummary,
    pub current_song_index: Option<usize>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlayerSummary {
    pub buzzer_id: String,
    pub name: String,
    pub score: i32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlaylistSummary {
    pub id: String,
    pub name: String,
    pub songs: Vec<SongSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SongSummary {
    pub id: String,
    pub starts_at_ms: u64,
    pub guess_duration_ms: u64,
    pub url: String,
    pub point_fields: Vec<PointFieldSummary>,
    pub bonus_fields: Vec<PointFieldSummary>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PointFieldSummary {
    pub key: String,
    pub value: String,
    pub points: i8,
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

impl From<Player> for PlayerSummary {
    fn from(player: Player) -> Self {
        Self {
            buzzer_id: player.buzzer_id,
            name: player.name,
            score: player.score,
        }
    }
}

impl From<(u32, Song)> for SongSummary {
    fn from((id, song): (u32, Song)) -> Self {
        Self {
            id: id.to_string(),
            starts_at_ms: song.start_time_ms,
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
            id: playlist.id.to_string(),
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
            created_at: session.created_at.to_string(),
            updated_at: session.updated_at.to_string(),
            players: session.players.into_iter().map(Into::into).collect(),
            playlist: playlist_summary,
            current_song_index: session.current_song_index,
        }
    }
}

fn ordered_song_summaries(
    playlist_songs: DashMap<u32, Song>,
    order: Vec<u32>,
) -> Result<Vec<SongSummary>, PlaylistOrderError> {
    let playlist_ids = playlist_songs
        .iter()
        .map(|entry| *entry.key())
        .collect::<HashSet<_>>();
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

            Ok((song_id, song_ref.value().clone()).into())
        })
        .collect::<Result<Vec<SongSummary>, _>>()
}
