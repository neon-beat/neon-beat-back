use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use uuid::Uuid;

/// Playlist definition containing a list of songs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaylistEntity {
    /// Stable identifier for the playlist.
    pub id: Uuid,
    /// Human readable playlist name.
    pub name: String,
    /// Set of songs that make up the game (key is the ID of the song).
    pub songs: Vec<SongEntity>,
}

/// Song entry inside a playlist.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SongEntity {
    /// Timestamp (milliseconds) where the song preview should start.
    pub starts_at_ms: usize,
    /// Allowed time (milliseconds) for teams to identify the song.
    pub guess_duration_ms: usize,
    /// URL pointing to the media resource.
    pub url: String,
    /// Fields required to award the base points (e.g., song title, artist).
    pub point_fields: Vec<PointFieldEntity>,
    /// Optional extra fields that can yield bonus points.
    pub bonus_fields: Vec<PointFieldEntity>,
}

/// Data for a point field associated to a song of a playlist.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PointFieldEntity {
    /// The name of the field to found (e.g. "Artist").
    pub key: String,
    /// The value to found for this field (e.g. the actual artist name).
    pub value: String,
    /// The number of points given if this field is found.
    pub points: u8,
}

/// Representation of a team stored in persistence and shared across layers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeamEntity {
    /// Stable identifier for the team.
    pub id: Uuid,
    /// Unique buzzer identifier (12 lowercase hexadecimal characters).
    pub buzzer_id: Option<String>,
    /// Display name chosen for the team.
    pub name: String,
    /// Current score for the team.
    pub score: i32,
    /// HSV color assigned to the team.
    pub color: TeamColorEntity,
    /// Last time this team was updated.
    pub updated_at: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamColorEntity {
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

impl PartialEq for TeamColorEntity {
    fn eq(&self, other: &Self) -> bool {
        self.h.to_bits() == other.h.to_bits()
            && self.s.to_bits() == other.s.to_bits()
            && self.v.to_bits() == other.v.to_bits()
    }
}

impl Eq for TeamColorEntity {}

/// Summary representation of a team stored in persistence and shared across layers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeamSummaryEntity {
    /// Stable identifier for the team.
    pub id: Uuid,
    /// Display name chosen for the team.
    pub name: String,
}

/// Aggregate game entity persisted by the storage layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameEntity {
    /// Primary key of the game.
    pub id: Uuid,
    /// Display name of the quiz / round.
    pub name: String,
    /// Creation timestamp for auditing/debugging.
    pub created_at: SystemTime,
    /// Last time the game entity was updated.
    pub updated_at: SystemTime,
    /// Participating teams and their current scores.
    pub teams: Vec<TeamEntity>,
    /// ID of the playlist used in this game session.
    pub playlist_id: Uuid,
    /// Oredered list of songs IDs from the playlist, defining the playlist order.
    pub playlist_song_order: Vec<u32>,
    /// Index of the current song to be found.
    pub current_song_index: Option<usize>,
    /// Whether the current song has already been revealed.
    pub current_song_found: bool,
}

/// Aggregate game list item entity (subset of GameEntity) persisted by the storage layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameListItemEntity {
    /// Primary key of the game.
    pub id: Uuid,
    /// Display name of the quiz / round.
    pub name: String,
    /// Creation timestamp for auditing/debugging.
    pub created_at: SystemTime,
    /// Last time the game entity was updated.
    pub updated_at: SystemTime,
    /// Participating teams.
    pub teams: Vec<TeamSummaryEntity>,
    /// ID of the playlist used in this game session.
    pub playlist_id: Uuid,
}

impl From<TeamEntity> for TeamSummaryEntity {
    fn from(value: TeamEntity) -> Self {
        Self {
            id: value.id,
            name: value.name,
        }
    }
}

impl From<GameEntity> for GameListItemEntity {
    fn from(entity: GameEntity) -> Self {
        Self {
            id: entity.id,
            name: entity.name,
            created_at: entity.created_at,
            updated_at: entity.updated_at,
            teams: entity.teams.into_iter().map(Into::into).collect(),
            playlist_id: entity.playlist_id,
        }
    }
}
