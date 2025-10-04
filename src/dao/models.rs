use std::collections::HashMap;

use mongodb::bson::DateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Playlist definition containing a list of songs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlaylistEntity {
    /// Stable identifier for the playlist.
    pub id: Uuid,
    /// Human readable playlist name.
    pub name: String,
    /// Set of songs that make up the game (key is the ID of the song).
    pub songs: HashMap<u32, SongEntity>,
}

/// Song entry inside a playlist.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SongEntity {
    /// Timestamp (milliseconds) where the song preview should start.
    pub starts_at_ms: u64,
    /// Allowed time (milliseconds) for teams to identify the song.
    pub guess_duration_ms: u64,
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
    pub points: i8,
}

/// Representation of a player stored in MongoDB and shared across layers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlayerEntity {
    /// Unique buzzer identifier (12 lowercase hexadecimal characters).
    pub buzzer_id: String,
    /// Display name chosen for the player/team.
    pub name: String,
    /// Current score for the player.
    pub score: i32,
}

/// Aggregate game entity stored in MongoDB.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameEntity {
    /// Primary key of the game.
    #[serde(rename = "_id")]
    pub id: Uuid,
    /// Display name of the quiz / round.
    pub name: String,
    /// Creation timestamp for auditing/debugging.
    pub created_at: DateTime,
    /// Last time the game entity was updated.
    pub updated_at: DateTime,
    /// Participating players and their current scores.
    pub players: Vec<PlayerEntity>,
    /// ID of the playlist used in this game session.
    pub playlist_id: Uuid,
    /// Oredered list of songs IDs from the playlist, defining the playlist order.
    pub playlist_song_order: Vec<u32>,
    /// Index of the current song to be found.
    pub current_song_index: Option<usize>,
}

impl GameEntity {
    /// Update the modification timestamp.
    pub fn touch(&mut self) {
        self.updated_at = DateTime::now();
    }
}
