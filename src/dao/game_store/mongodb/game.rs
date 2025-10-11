use futures::{TryStreamExt, future::BoxFuture};
use mongodb::{
    Collection, Database,
    bson::{Binary, DateTime, doc, spec::BinarySubtype},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::dao::{
    game_store::GameStore,
    models::{GameEntity, PlayerEntity, PlaylistEntity},
    storage::{StorageError, StorageResult},
};

use super::{MongoDaoError, MongoManager};

const GAME_COLLECTION_NAME: &str = "games";
const PLAYLIST_COLLECTION_NAME: &str = "playlists";

/// MongoDB-backed [`GameStore`] implementation.
#[derive(Clone)]
pub struct MongoGameStore {
    mongo: MongoManager,
}

impl MongoGameStore {
    pub fn new(mongo: MongoManager) -> Self {
        Self { mongo }
    }

    async fn collection(&self) -> mongodb::error::Result<Collection<MongoGameDocument>> {
        let database: Database = self.mongo.database().await;
        Ok(database.collection::<MongoGameDocument>(GAME_COLLECTION_NAME))
    }

    async fn playlist_collection(&self) -> mongodb::error::Result<Collection<PlaylistEntity>> {
        let database: Database = self.mongo.database().await;
        Ok(database.collection::<PlaylistEntity>(PLAYLIST_COLLECTION_NAME))
    }

    async fn save_game(&self, game: GameEntity) -> Result<(), MongoDaoError> {
        let id = game.id;
        let document: MongoGameDocument = game.into();
        let collection = self
            .collection()
            .await
            .map_err(|source| MongoDaoError::SaveGame { id, source })?;

        collection
            .replace_one(doc! {"_id": uuid_as_binary(id)}, &document)
            .upsert(true)
            .await
            .map_err(|source| MongoDaoError::SaveGame { id, source })?;

        Ok(())
    }

    async fn save_playlist_entity(&self, playlist: PlaylistEntity) -> Result<(), MongoDaoError> {
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

    async fn find_game(&self, id: Uuid) -> Result<Option<GameEntity>, MongoDaoError> {
        let collection = self
            .collection()
            .await
            .map_err(|source| MongoDaoError::LoadGame { id, source })?;

        let document = collection
            .find_one(doc! {"_id": uuid_as_binary(id)})
            .await
            .map_err(|source| MongoDaoError::LoadGame { id, source })?;

        Ok(document.map(Into::into))
    }

    async fn find_playlist_entity(
        &self,
        id: Uuid,
    ) -> Result<Option<PlaylistEntity>, MongoDaoError> {
        let collection = self
            .playlist_collection()
            .await
            .map_err(|source| MongoDaoError::LoadPlaylist { id, source })?;

        collection
            .find_one(doc! {"_id": uuid_as_binary(id)})
            .await
            .map_err(|source| MongoDaoError::LoadPlaylist { id, source })
    }

    async fn list_games_internal(&self) -> Result<Vec<(Uuid, String)>, MongoDaoError> {
        let collection = self
            .collection()
            .await
            .map_err(|source| MongoDaoError::ListGames { source })?;

        let docs: Vec<MongoGameDocument> = collection
            .find(doc! {})
            .await
            .map_err(|source| MongoDaoError::ListGames { source })?
            .try_collect()
            .await
            .map_err(|source| MongoDaoError::ListGames { source })?;

        Ok(docs
            .into_iter()
            .map(|doc| {
                let game: GameEntity = doc.into();
                (game.id, game.name)
            })
            .collect())
    }

    async fn list_playlists_internal(&self) -> Result<Vec<(Uuid, String)>, MongoDaoError> {
        let collection = self
            .playlist_collection()
            .await
            .map_err(|source| MongoDaoError::ListPlaylists { source })?;

        let docs: Vec<PlaylistEntity> = collection
            .find(doc! {})
            .await
            .map_err(|source| MongoDaoError::ListPlaylists { source })?
            .try_collect()
            .await
            .map_err(|source| MongoDaoError::ListPlaylists { source })?;

        Ok(docs
            .into_iter()
            .map(|playlist| (playlist.id, playlist.name))
            .collect())
    }
}

impl GameStore for MongoGameStore {
    fn save_game(&self, game: GameEntity) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move { store.save_game(game).await.map_err(Into::into) })
    }

    fn save_playlist(&self, playlist: PlaylistEntity) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move {
            store
                .save_playlist_entity(playlist)
                .await
                .map_err(Into::into)
        })
    }

    fn find_game(&self, id: Uuid) -> BoxFuture<'static, StorageResult<Option<GameEntity>>> {
        let store = self.clone();
        Box::pin(async move { store.find_game(id).await.map_err(Into::into) })
    }

    fn find_playlist(&self, id: Uuid) -> BoxFuture<'static, StorageResult<Option<PlaylistEntity>>> {
        let store = self.clone();
        Box::pin(async move { store.find_playlist_entity(id).await.map_err(Into::into) })
    }

    fn list_games(&self) -> BoxFuture<'static, StorageResult<Vec<(Uuid, String)>>> {
        let store = self.clone();
        Box::pin(async move { store.list_games_internal().await.map_err(Into::into) })
    }

    fn list_playlists(&self) -> BoxFuture<'static, StorageResult<Vec<(Uuid, String)>>> {
        let store = self.clone();
        Box::pin(async move { store.list_playlists_internal().await.map_err(Into::into) })
    }

    fn health_check(&self) -> BoxFuture<'static, StorageResult<()>> {
        let mongo = self.mongo.clone();
        Box::pin(async move { mongo.ping().await.map_err(Into::into) })
    }
}

impl From<MongoDaoError> for StorageError {
    fn from(err: MongoDaoError) -> Self {
        StorageError::unavailable(err.to_string(), err)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MongoGameDocument {
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
