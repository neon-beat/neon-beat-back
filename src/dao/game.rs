use mongodb::{
    Collection, Database,
    bson::{Binary, doc, spec::BinarySubtype},
};
use uuid::Uuid;

use crate::dao::{
    models::{GameEntity, PlaylistEntity},
    mongodb::MongoManager,
};

use super::mongodb::MongoDaoError;

const GAME_COLLECTION_NAME: &str = "games";
const PLAYLIST_COLLECTION_NAME: &str = "playlists";

/// Data Access Object encapsulating MongoDB interaction for game entities.
#[derive(Clone)]
pub struct GameRepository {
    mongo: MongoManager,
}

impl GameRepository {
    pub fn new(mongo: MongoManager) -> Self {
        Self { mongo }
    }

    async fn collection(&self) -> mongodb::error::Result<Collection<GameEntity>> {
        let database: Database = self.mongo.database().await;
        Ok(database.collection::<GameEntity>(GAME_COLLECTION_NAME))
    }

    async fn playlist_collection(&self) -> mongodb::error::Result<Collection<PlaylistEntity>> {
        let database: Database = self.mongo.database().await;
        Ok(database.collection::<PlaylistEntity>(PLAYLIST_COLLECTION_NAME))
    }

    /// Upsert a game entity, replacing any previous state with the provided payload.
    pub async fn save(&self, game: GameEntity) -> Result<(), MongoDaoError> {
        let collection = self
            .collection()
            .await
            .map_err(|source| MongoDaoError::SaveGame {
                id: game.id,
                source,
            })?;

        collection
            .replace_one(doc! {"_id": uuid_as_binary(game.id)}, &game)
            .upsert(true)
            .await
            .map_err(|source| MongoDaoError::SaveGame {
                id: game.id,
                source,
            })?;

        Ok(())
    }

    /// Upsert a playlist entity in the dedicated collection so curated track
    /// lists can be reused across games.
    pub async fn save_playlist(&self, playlist: PlaylistEntity) -> Result<(), MongoDaoError> {
        let collection =
            self.playlist_collection()
                .await
                .map_err(|source| MongoDaoError::SavePlaylist {
                    id: playlist.id,
                    source,
                })?;

        collection
            .replace_one(doc! {"_id": uuid_as_binary(playlist.id)}, &playlist)
            .upsert(true)
            .await
            .map_err(|source| MongoDaoError::SavePlaylist {
                id: playlist.id,
                source,
            })?;

        Ok(())
    }

    /// Fetch a game entity by id.
    pub async fn find(&self, id: Uuid) -> Result<Option<GameEntity>, MongoDaoError> {
        let collection = self
            .collection()
            .await
            .map_err(|source| MongoDaoError::LoadGame { id, source })?;

        collection
            .find_one(doc! {"_id": uuid_as_binary(id)})
            .await
            .map_err(|source| MongoDaoError::LoadGame { id, source })
    }

    /// Fetch a playlist entity by id from the shared playlist collection.
    pub async fn find_playlist(&self, id: Uuid) -> Result<Option<PlaylistEntity>, MongoDaoError> {
        let collection = self
            .playlist_collection()
            .await
            .map_err(|source| MongoDaoError::LoadPlaylist { id, source })?;

        collection
            .find_one(doc! {"_id": uuid_as_binary(id)})
            .await
            .map_err(|source| MongoDaoError::LoadPlaylist { id, source })
    }
}

fn uuid_as_binary(id: Uuid) -> Binary {
    Binary {
        subtype: BinarySubtype::Uuid,
        bytes: id.into_bytes().to_vec(),
    }
}
