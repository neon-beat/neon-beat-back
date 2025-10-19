use std::{collections::HashMap, time::SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::dao::models::{GameEntity, PlaylistEntity, SongEntity, TeamEntity};

pub const GAME_PREFIX: &str = "game::";
pub const PLAYLIST_PREFIX: &str = "playlist::";
pub const END_SUFFIX: &str = "\u{ffff}";

#[derive(Debug, Deserialize)]
pub struct AllDocsResponse {
    pub rows: Vec<AllDocsRow>,
}

#[derive(Debug, Deserialize)]
pub struct AllDocsRow {
    pub id: String,
    #[serde(default)]
    pub doc: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouchGameDocument {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_rev", skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    #[serde(flatten)]
    pub game: GameBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameBody {
    pub name: String,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
    pub teams: Vec<TeamEntity>,
    pub playlist_id: Uuid,
    pub playlist_song_order: Vec<u32>,
    pub current_song_index: Option<usize>,
}

impl CouchGameDocument {
    pub fn from_entity(value: GameEntity) -> Self {
        Self {
            id: game_doc_id(value.id),
            rev: None,
            game: GameBody {
                name: value.name,
                created_at: value.created_at,
                updated_at: value.updated_at,
                teams: value.teams,
                playlist_id: value.playlist_id,
                playlist_song_order: value.playlist_song_order,
                current_song_index: value.current_song_index,
            },
        }
    }

    pub fn into_entity(self) -> GameEntity {
        GameEntity {
            id: extract_uuid(&self.id).unwrap_or_else(Uuid::nil),
            name: self.game.name,
            created_at: self.game.created_at,
            updated_at: self.game.updated_at,
            teams: self.game.teams,
            playlist_id: self.game.playlist_id,
            playlist_song_order: self.game.playlist_song_order,
            current_song_index: self.game.current_song_index,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouchPlaylistDocument {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_rev", skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    #[serde(flatten)]
    pub playlist: PlaylistBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistBody {
    pub name: String,
    #[serde(with = "super::song_map")]
    pub songs: HashMap<u32, SongEntity>,
}

impl CouchPlaylistDocument {
    pub fn from_entity(value: PlaylistEntity) -> Self {
        Self {
            id: playlist_doc_id(value.id),
            rev: None,
            playlist: PlaylistBody {
                name: value.name,
                songs: value.songs,
            },
        }
    }

    pub fn into_entity(self) -> PlaylistEntity {
        PlaylistEntity {
            id: extract_uuid(&self.id).unwrap_or_else(Uuid::nil),
            name: self.playlist.name,
            songs: self.playlist.songs,
        }
    }
}

pub fn game_doc_id(id: Uuid) -> String {
    format!("{}{}", GAME_PREFIX, id)
}

pub fn playlist_doc_id(id: Uuid) -> String {
    format!("{}{}", PLAYLIST_PREFIX, id)
}

pub fn extract_uuid(doc_id: &str) -> Option<Uuid> {
    doc_id
        .split_once("::")
        .and_then(|(_, id)| Uuid::parse_str(id).ok())
}
