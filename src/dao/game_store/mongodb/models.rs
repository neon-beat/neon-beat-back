use mongodb::bson::{Binary, DateTime, Document, doc, spec::BinarySubtype};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::dao::models::{GameEntity, PlayerEntity};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MongoGameDocument {
    #[serde(rename = "_id")]
    id: Uuid,
    name: String,
    created_at: DateTime,
    updated_at: DateTime,
    players: Vec<PlayerEntity>,
    playlist_id: Uuid,
    playlist_song_order: Vec<u32>,
    current_song_index: Option<usize>,
}

impl From<GameEntity> for MongoGameDocument {
    fn from(value: GameEntity) -> Self {
        Self {
            id: value.id,
            name: value.name,
            created_at: DateTime::from_system_time(value.created_at),
            updated_at: DateTime::from_system_time(value.updated_at),
            players: value.players,
            playlist_id: value.playlist_id,
            playlist_song_order: value.playlist_song_order,
            current_song_index: value.current_song_index,
        }
    }
}

impl From<MongoGameDocument> for GameEntity {
    fn from(value: MongoGameDocument) -> Self {
        Self {
            id: value.id,
            name: value.name,
            created_at: value.created_at.to_system_time(),
            updated_at: value.updated_at.to_system_time(),
            players: value.players,
            playlist_id: value.playlist_id,
            playlist_song_order: value.playlist_song_order,
            current_song_index: value.current_song_index,
        }
    }
}

fn uuid_as_binary(id: Uuid) -> Binary {
    Binary {
        subtype: BinarySubtype::Uuid,
        bytes: id.into_bytes().to_vec(),
    }
}

pub fn doc_id(id: Uuid) -> Document {
    doc! {"_id": uuid_as_binary(id)}
}
