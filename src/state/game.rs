use indexmap::IndexMap;
use std::time::SystemTime;
use uuid::Uuid;

use crate::{
    dao::models::{
        GameEntity, PlaylistEntity, PointFieldEntity, SongEntity, TeamEntity, TeamSummaryEntity,
    },
    dto::game::TeamBriefSummary,
};

/// Runtime representation of a playlist with its songs keyed by identifier.
#[derive(Debug, Clone)]
pub struct Playlist {
    /// Stable identifier for the playlist.
    pub id: Uuid,
    /// Human readable playlist name.
    pub name: String,
    /// Set of songs that make up the game (key is the ID of the song).
    pub songs: IndexMap<u32, Song>,
}

/// Metadata for a song of a playlist.
#[derive(Debug, Clone)]
pub struct Song {
    /// Timestamp (milliseconds) where the song preview should start.
    pub starts_at_ms: usize,
    /// Allowed time (milliseconds) for teams to identify the song.
    pub guess_duration_ms: usize,
    /// URL pointing to the media resource.
    pub url: String,
    /// Fields required to award the base points (e.g., song title, artist).
    pub point_fields: Vec<PointField>,
    /// Optional extra fields that can yield bonus points.
    pub bonus_fields: Vec<PointField>,
}

/// Data for a point field associated to a song of a playlist.
#[derive(Debug, Clone)]
pub struct PointField {
    /// The name of the field to found (e.g. "Artist").
    pub key: String,
    /// The value to found for this field (e.g. the actual artist name).
    pub value: String,
    /// The number of points given if this field is found.
    pub points: u8,
}

/// Team info tracked during a game session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Team {
    /// Stable identifier for this team.
    pub id: Uuid,
    /// Unique buzzer identifier (12 lowercase hexadecimal characters).
    pub buzzer_id: Option<String>,
    /// Display name chosen for the team.
    pub name: String,
    /// Current score for the team.
    pub score: i32,
}

/// Aggregated state for an in-progress or persisted game session.
#[derive(Debug, Clone)]
pub struct GameSession {
    /// Primary key of the game.
    pub id: Uuid,
    /// Display name of the quiz / round.
    pub name: String,
    /// Creation timestamp for auditing/debugging.
    pub created_at: SystemTime,
    /// Last time the game document was updated.
    pub updated_at: SystemTime,
    /// Participating teams and their current scores.
    pub teams: Vec<Team>,
    /// Playlist selected for this session.
    pub playlist: Playlist,
    /// Oredered list of songs IDs from the playlist, defining the playlist order.
    pub playlist_song_order: Vec<u32>,
    /// Index of the current song to be found.
    pub current_song_index: Option<usize>,
    /// Field names (key) already found for the current song.
    pub found_point_fields: Vec<String>,
    /// Bonus field names (key) found for the current song.
    pub found_bonus_fields: Vec<String>,
}

impl GameSession {
    /// Build a new in-memory session with the provided metadata.
    pub fn new(name: String, teams: Vec<Team>, playlist: Playlist) -> Self {
        let timestamp = SystemTime::now();

        let playlist_song_order: Vec<u32> = playlist.songs.keys().cloned().collect();

        Self {
            id: Uuid::new_v4(),
            name,
            created_at: timestamp,
            updated_at: timestamp,
            teams,
            playlist,
            playlist_song_order,
            current_song_index: Some(0),
            found_point_fields: Vec::new(),
            found_bonus_fields: Vec::new(),
        }
    }

    /// Return the song at the requested playlist index together with its identifier.
    pub fn get_song(&self, index: usize) -> Option<(u32, Song)> {
        self.playlist_song_order.get(index).and_then(|song_id| {
            self.playlist
                .songs
                .get(song_id)
                .map(|song| (*song_id, song.clone()))
        })
    }
}

impl Playlist {
    /// Build a new in-memory playlist with the provided metadata, allocating a
    /// fresh unique identifier for runtime usage.
    pub fn new(name: String, songs: IndexMap<u32, Song>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            songs,
        }
    }
}

impl From<PointFieldEntity> for PointField {
    fn from(value: PointFieldEntity) -> Self {
        Self {
            key: value.key,
            value: value.value,
            points: value.points,
        }
    }
}

impl From<PointField> for PointFieldEntity {
    fn from(value: PointField) -> Self {
        Self {
            key: value.key,
            value: value.value,
            points: value.points,
        }
    }
}

impl From<SongEntity> for Song {
    fn from(value: SongEntity) -> Self {
        Self {
            starts_at_ms: value.starts_at_ms,
            guess_duration_ms: value.guess_duration_ms,
            url: value.url,
            point_fields: value.point_fields.into_iter().map(Into::into).collect(),
            bonus_fields: value.bonus_fields.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<Song> for SongEntity {
    fn from(value: Song) -> Self {
        Self {
            starts_at_ms: value.starts_at_ms,
            guess_duration_ms: value.guess_duration_ms,
            url: value.url,
            point_fields: value.point_fields.into_iter().map(Into::into).collect(),
            bonus_fields: value.bonus_fields.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<PlaylistEntity> for Playlist {
    fn from(value: PlaylistEntity) -> Self {
        Self {
            id: value.id,
            name: value.name,
            songs: value
                .songs
                .into_iter()
                .enumerate()
                .map(|(id, se)| (id as u32, se.into()))
                .collect(),
        }
    }
}

impl From<Playlist> for PlaylistEntity {
    fn from(value: Playlist) -> Self {
        Self {
            id: value.id,
            name: value.name,
            songs: value.songs.into_values().map(Into::into).collect(),
        }
    }
}

impl From<TeamEntity> for Team {
    fn from(value: TeamEntity) -> Self {
        Self {
            id: value.id,
            buzzer_id: value.buzzer_id,
            name: value.name,
            score: value.score,
        }
    }
}

impl From<Team> for TeamEntity {
    fn from(value: Team) -> Self {
        Self {
            id: value.id,
            buzzer_id: value.buzzer_id,
            name: value.name,
            score: value.score,
        }
    }
}

impl From<TeamSummaryEntity> for TeamBriefSummary {
    fn from(value: TeamSummaryEntity) -> Self {
        Self {
            id: value.id,
            name: value.name,
        }
    }
}

impl From<(GameEntity, PlaylistEntity)> for GameSession {
    fn from((game, playlist): (GameEntity, PlaylistEntity)) -> Self {
        Self {
            id: game.id,
            name: game.name,
            created_at: game.created_at,
            updated_at: game.updated_at,
            teams: game.teams.into_iter().map(Into::into).collect(),
            playlist: playlist.into(),
            playlist_song_order: game.playlist_song_order,
            current_song_index: game.current_song_index,
            found_point_fields: Vec::new(),
            found_bonus_fields: Vec::new(),
        }
    }
}

impl From<GameSession> for GameEntity {
    fn from(value: GameSession) -> Self {
        Self {
            id: value.id,
            name: value.name,
            created_at: value.created_at,
            updated_at: value.updated_at,
            teams: value.teams.into_iter().map(Into::into).collect(),
            playlist_id: value.playlist.id,
            playlist_song_order: value.playlist_song_order,
            current_song_index: value.current_song_index,
        }
    }
}
