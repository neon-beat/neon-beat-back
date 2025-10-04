use dashmap::DashMap;
use mongodb::bson::DateTime;
use rand::{seq::SliceRandom, thread_rng};
use uuid::Uuid;

use crate::dao::models::{GameEntity, PlayerEntity, PlaylistEntity, PointFieldEntity, SongEntity};

/// Runtime representation of a playlist with its songs keyed by identifier.
#[derive(Debug, Clone)]
pub struct Playlist {
    /// Stable identifier for the playlist.
    pub id: Uuid,
    /// Human readable playlist name.
    pub name: String,
    /// Set of songs that make up the game (key is the ID of the song).
    pub songs: DashMap<u32, Song>,
}

/// Metadata for a song of a playlist.
#[derive(Debug, Clone)]
pub struct Song {
    /// Timestamp (milliseconds) where the song preview should start.
    pub start_time_ms: u64,
    /// Allowed time (milliseconds) for teams to identify the song.
    pub guess_duration_ms: u64,
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
    pub points: i8,
}

/// Player info tracked during a game session.
#[derive(Debug, Clone)]
pub struct Player {
    /// Unique buzzer identifier (12 lowercase hexadecimal characters).
    pub buzzer_id: String,
    /// Display name chosen for the player/team.
    pub name: String,
    /// Current score for the player.
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
    pub created_at: DateTime,
    /// Last time the game document was updated.
    pub updated_at: DateTime,
    /// Participating players and their current scores.
    pub players: Vec<Player>,
    /// Playlist selected for this session.
    pub playlist: Playlist,
    /// Oredered list of songs IDs from the playlist, defining the playlist order.
    pub playlist_song_order: Vec<u32>,
    /// Index of the current song to be found.
    pub current_song_index: Option<usize>,
}

impl GameSession {
    /// Build a new in-memory session with the provided metadata.
    ///
    /// The playlist order is shuffled once using the playlist song ids so a
    /// fresh game starts with a randomized sequence while keeping deterministic
    /// identifiers for persistence and DTO conversions.
    pub fn new(name: String, players: Vec<Player>, playlist: Playlist) -> Self {
        let timestamp = DateTime::now();

        let mut playlist_song_order: Vec<u32> =
            playlist.songs.iter().map(|entry| *entry.key()).collect();

        if playlist_song_order.len() > 1 {
            let mut rng = thread_rng();
            playlist_song_order.shuffle(&mut rng);
        }

        Self {
            id: Uuid::new_v4(),
            name,
            created_at: timestamp,
            updated_at: timestamp,
            players,
            playlist,
            playlist_song_order,
            current_song_index: None,
        }
    }
}

impl Playlist {
    /// Build a new in-memory playlist with the provided metadata, allocating a
    /// fresh unique identifier for runtime usage.
    pub fn new(name: String, songs: DashMap<u32, Song>) -> Self {
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
            start_time_ms: value.starts_at_ms,
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
            starts_at_ms: value.start_time_ms,
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
                .map(|(id, se)| (id, se.into()))
                .collect(),
        }
    }
}

impl From<Playlist> for PlaylistEntity {
    fn from(value: Playlist) -> Self {
        Self {
            id: value.id,
            name: value.name,
            songs: value
                .songs
                .into_iter()
                .map(|(id, se)| (id, se.into()))
                .collect(),
        }
    }
}

impl From<PlayerEntity> for Player {
    fn from(value: PlayerEntity) -> Self {
        Self {
            buzzer_id: value.buzzer_id,
            name: value.name,
            score: value.score,
        }
    }
}

impl From<Player> for PlayerEntity {
    fn from(value: Player) -> Self {
        Self {
            buzzer_id: value.buzzer_id,
            name: value.name,
            score: value.score,
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
            players: game.players.into_iter().map(Into::into).collect(),
            playlist: playlist.into(),
            playlist_song_order: game.playlist_song_order,
            current_song_index: game.current_song_index,
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
            players: value.players.into_iter().map(Into::into).collect(),
            playlist_id: value.playlist.id,
            playlist_song_order: value.playlist_song_order,
            current_song_index: value.current_song_index,
        }
    }
}
