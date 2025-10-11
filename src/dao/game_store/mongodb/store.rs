use std::{sync::Arc, time::Duration};

use futures::{TryStreamExt, future::BoxFuture};
use mongodb::{
    Client, Collection, Database,
    bson::doc,
    options::{ClientOptions, IndexOptions},
};
use tokio::{
    sync::RwLock,
    time::{MissedTickBehavior, interval},
};
use tracing::{info, warn};
use uuid::Uuid;

use super::{
    error::{MongoDaoError, MongoResult},
    models::{MongoGameDocument, doc_id},
};
use crate::dao::{
    game_store::{GameStore, mongodb::connection::establish_connection},
    models::{GameEntity, PlaylistEntity},
    storage::StorageResult,
};

const GAME_COLLECTION_NAME: &str = "games";
const PLAYLIST_COLLECTION_NAME: &str = "playlists";
const DEFAULT_DB: &str = "neon_beat";
const HEALTH_CHECK_INTERVAL_SECS: u64 = 30;

#[derive(Clone)]
pub struct MongoGameStore {
    state: Arc<RwLock<MongoState>>,
    config: Arc<MongoConfig>,
}

struct MongoState {
    client: Client,
    database: Database,
}

struct MongoConfig {
    options: ClientOptions,
    database_name: String,
}

impl MongoGameStore {
    /// Establish a connection to MongoDB and ensure indexes are present.
    pub async fn connect(uri: &str, db_name: Option<&str>) -> MongoResult<Self> {
        let database_name = db_name.unwrap_or(DEFAULT_DB).to_owned();
        let options =
            ClientOptions::parse(uri)
                .await
                .map_err(|source| MongoDaoError::InvalidUri {
                    uri: uri.to_owned(),
                    source,
                })?;

        let (client, database) = establish_connection(&options, &database_name).await?;
        let state = Arc::new(RwLock::new(MongoState { client, database }));
        let config = Arc::new(MongoConfig {
            options,
            database_name,
        });
        let store = Self { state, config };

        store.ensure_indexes().await?;
        store.spawn_health_task();
        Ok(store)
    }

    async fn ensure_indexes(&self) -> MongoResult<()> {
        let database = self.database().await;
        let collection = database.collection::<mongodb::bson::Document>(GAME_COLLECTION_NAME);
        let index = mongodb::IndexModel::builder()
            .keys(doc! {"name": 1})
            .options(
                IndexOptions::builder()
                    .name(Some("game_name_idx".to_owned()))
                    .build(),
            )
            .build();

        collection
            .create_index(index)
            .await
            .map_err(|source| MongoDaoError::EnsureIndex {
                collection: GAME_COLLECTION_NAME,
                index: "name",
                source,
            })?;

        Ok(())
    }

    async fn database(&self) -> Database {
        let guard = self.state.read().await;
        guard.database.clone()
    }

    async fn collection(&self) -> Collection<MongoGameDocument> {
        let guard = self.state.read().await;
        guard
            .database
            .collection::<MongoGameDocument>(GAME_COLLECTION_NAME)
    }

    async fn playlist_collection(&self) -> Collection<PlaylistEntity> {
        let guard = self.state.read().await;
        guard
            .database
            .collection::<PlaylistEntity>(PLAYLIST_COLLECTION_NAME)
    }

    async fn save_game(&self, game: GameEntity) -> MongoResult<()> {
        let id = game.id;
        let document: MongoGameDocument = game.into();
        let collection = self.collection().await;

        collection
            .replace_one(doc_id(id), &document)
            .upsert(true)
            .await
            .map_err(|source| MongoDaoError::SaveGame { id, source })?;

        Ok(())
    }

    async fn save_playlist_entity(&self, playlist: PlaylistEntity) -> MongoResult<()> {
        let collection = self.playlist_collection().await;

        collection
            .replace_one(doc_id(playlist.id), &playlist)
            .upsert(true)
            .await
            .map_err(|source| MongoDaoError::SavePlaylist {
                id: playlist.id,
                source,
            })?;

        Ok(())
    }

    async fn find_game(&self, id: Uuid) -> MongoResult<Option<GameEntity>> {
        let collection = self.collection().await;

        let document = collection
            .find_one(doc_id(id))
            .await
            .map_err(|source| MongoDaoError::LoadGame { id, source })?;

        Ok(document.map(Into::into))
    }

    async fn find_playlist_entity(&self, id: Uuid) -> MongoResult<Option<PlaylistEntity>> {
        let collection = self.playlist_collection().await;

        collection
            .find_one(doc_id(id))
            .await
            .map_err(|source| MongoDaoError::LoadPlaylist { id, source })
    }

    async fn list_games_internal(&self) -> MongoResult<Vec<(Uuid, String)>> {
        let collection = self.collection().await;

        let documents: Vec<MongoGameDocument> = collection
            .find(doc! {})
            .await
            .map_err(|source| MongoDaoError::ListGames { source })?
            .try_collect()
            .await
            .map_err(|source| MongoDaoError::ListGames { source })?;

        Ok(documents
            .into_iter()
            .map(|doc| {
                let entity: GameEntity = doc.into();
                (entity.id, entity.name)
            })
            .collect())
    }

    async fn list_playlists_internal(&self) -> MongoResult<Vec<(Uuid, String)>> {
        let collection = self.playlist_collection().await;

        let documents: Vec<PlaylistEntity> = collection
            .find(doc! {})
            .await
            .map_err(|source| MongoDaoError::ListPlaylists { source })?
            .try_collect()
            .await
            .map_err(|source| MongoDaoError::ListPlaylists { source })?;

        Ok(documents
            .into_iter()
            .map(|playlist| (playlist.id, playlist.name))
            .collect())
    }

    async fn ping_once(state: &Arc<RwLock<MongoState>>) -> MongoResult<()> {
        let database = {
            let guard = state.read().await;
            guard.database.clone()
        };

        database
            .run_command(doc! { "ping": 1 })
            .await
            .map_err(|source| MongoDaoError::HealthPing { source })?;
        Ok(())
    }

    async fn reconnect(
        state: &Arc<RwLock<MongoState>>,
        options: &ClientOptions,
        database_name: &str,
    ) -> MongoResult<()> {
        let (client, database) = establish_connection(options, database_name).await?;
        let mut guard = state.write().await;
        guard.client = client;
        guard.database = database;
        Ok(())
    }

    fn spawn_health_task(&self) {
        let state = Arc::downgrade(&self.state);
        let config = self.config.clone();

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(HEALTH_CHECK_INTERVAL_SECS));
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

            loop {
                ticker.tick().await;

                let Some(state_arc) = state.upgrade() else {
                    break;
                };

                if let Err(err) = Self::ping_once(&state_arc).await {
                    warn!(error = %err, "MongoDB health check failed; attempting reconnect");
                    match Self::reconnect(&state_arc, &config.options, &config.database_name).await
                    {
                        Ok(()) => info!("MongoDB connection re-established"),
                        Err(err) => warn!(error = %err, "failed to reconnect to MongoDB"),
                    }
                }
            }
        });
    }

    async fn ping(&self) -> MongoResult<()> {
        Self::ping_once(&self.state).await
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
        let store = self.clone();
        Box::pin(async move { store.ping().await.map_err(Into::into) })
    }
}
