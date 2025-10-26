use std::{collections::HashMap, sync::Arc};

use futures::{TryStreamExt, future::BoxFuture};
use mongodb::{Client, Collection, Database, bson::doc, options::IndexOptions};
use tokio::sync::RwLock;
use uuid::Uuid;

use super::{
    config::MongoConfig,
    connection::establish_connection,
    error::{MongoDaoError, MongoResult},
    models::{MongoGameDocument, MongoTeamDocument, doc_id, uuid_as_binary},
};
use crate::dao::{
    game_store::GameStore,
    models::{GameEntity, GameListItemEntity, PlaylistEntity, TeamEntity},
    storage::StorageResult,
};

const GAME_COLLECTION_NAME: &str = "games";
const PLAYLIST_COLLECTION_NAME: &str = "playlists";

#[derive(Clone)]
pub struct MongoGameStore {
    inner: Arc<MongoInner>,
}

struct MongoInner {
    state: RwLock<MongoState>,
    config: MongoConfig,
}

struct MongoState {
    client: Client,
    database: Database,
}

impl MongoInner {
    async fn ping(&self) -> MongoResult<()> {
        let database = {
            let guard: tokio::sync::RwLockReadGuard<'_, MongoState> = self.state.read().await;
            guard.database.clone()
        };

        database
            .run_command(doc! { "ping": 1 })
            .await
            .map_err(|source| MongoDaoError::HealthPing { source })?;
        Ok(())
    }

    async fn reconnect(&self) -> MongoResult<()> {
        let (client, database) =
            establish_connection(&self.config.options, &self.config.database_name).await?;
        let mut guard = self.state.write().await;
        guard.client = client;
        guard.database = database;
        Ok(())
    }
}

impl MongoGameStore {
    /// Establish a connection to MongoDB and ensure indexes are present.
    pub async fn connect(config: MongoConfig) -> MongoResult<Self> {
        let (client, database) =
            establish_connection(&config.options, &config.database_name).await?;

        let inner = Arc::new(MongoInner {
            state: RwLock::new(MongoState { client, database }),
            config,
        });

        let store = Self { inner };
        store.ensure_indexes().await?;
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

        // Ensure index on teams collection for efficient lookups by (game_id, team_id)
        let team_collection = database.collection::<MongoTeamDocument>("teams");
        let team_index = mongodb::IndexModel::builder()
            .keys(doc! {"game_id": 1, "team_id": 1})
            .options(
                IndexOptions::builder()
                    .name(Some("team_game_idx".to_owned()))
                    .unique(Some(true))
                    .build(),
            )
            .build();

        team_collection
            .create_index(team_index)
            .await
            .map_err(|source| MongoDaoError::EnsureIndex {
                collection: "teams",
                index: "game_id,team_id",
                source,
            })?;

        Ok(())
    }

    async fn database(&self) -> Database {
        let guard = self.inner.state.read().await;
        guard.database.clone()
    }

    async fn collection(&self) -> Collection<MongoGameDocument> {
        let guard = self.inner.state.read().await;
        guard
            .database
            .collection::<MongoGameDocument>(GAME_COLLECTION_NAME)
    }

    async fn team_collection(&self) -> Collection<MongoTeamDocument> {
        let guard = self.inner.state.read().await;
        guard.database.collection::<MongoTeamDocument>("teams")
    }

    async fn playlist_collection(&self) -> Collection<PlaylistEntity> {
        let guard = self.inner.state.read().await;
        guard
            .database
            .collection::<PlaylistEntity>(PLAYLIST_COLLECTION_NAME)
    }

    /// Helper to persist the game document.
    /// Extracts team IDs from the GameEntity.
    async fn save_game_document(&self, game: GameEntity) -> MongoResult<()> {
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

    async fn save_game(&self, game: GameEntity) -> MongoResult<()> {
        let id = game.id;
        // First persist individual team documents in the teams collection.
        let team_coll = self.team_collection().await;
        for team in game.teams.iter() {
            let team_doc: MongoTeamDocument = (game.id, team.clone()).into();
            team_coll
                .replace_one(doc! { "game_id": uuid_as_binary(team_doc.game_id), "team_id": uuid_as_binary(team_doc.team_id) }, &team_doc)
                .upsert(true)
                .await
                .map_err(|source| MongoDaoError::SaveGame { id, source })?;
        }

        // Persist the game document (team IDs extracted from game.teams)
        self.save_game_document(game).await
    }

    async fn save_game_without_teams(&self, game: GameEntity) -> MongoResult<()> {
        // Persist the game document (team IDs extracted from game.teams)
        self.save_game_document(game).await
    }

    async fn delete_game(&self, id: Uuid) -> MongoResult<bool> {
        let collection = self.collection().await;
        let result = collection
            .delete_one(doc_id(id))
            .await
            .map_err(|source| MongoDaoError::DeleteGame { id, source })?;
        Ok(result.deleted_count > 0)
    }

    async fn save_playlist(&self, playlist: PlaylistEntity) -> MongoResult<()> {
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

        let maybe_doc = match document {
            Some(doc) => doc,
            None => return Ok(None),
        };

        // Load team documents for this game and assemble the GameEntity
        let team_coll = self.team_collection().await;
        let team_docs: Vec<MongoTeamDocument> = team_coll
            .find(doc! { "game_id": uuid_as_binary(id) })
            .await
            .map_err(|source| MongoDaoError::LoadGame { id, source })?
            .try_collect()
            .await
            .map_err(|source| MongoDaoError::LoadGame { id, source })?;

        // Build map from team_id to TeamEntity
        let mut team_map: HashMap<Uuid, TeamEntity> = HashMap::new();
        for td in team_docs {
            let (_tid, team_entity) = (td)
                .try_into()
                .map_err(|e| MongoDaoError::LoadGame { id, source: e })?;
            team_map.insert(team_entity.id, team_entity);
        }

        // Order teams according to the game document's team id list
        let team_ids = maybe_doc.teams.clone();
        let teams = team_ids
            .into_iter()
            .filter_map(|team_id| team_map.get(&team_id).cloned())
            .collect();

        let mut game_entity: GameEntity = maybe_doc.into();
        game_entity.teams = teams;
        Ok(Some(game_entity))
    }

    async fn find_playlist(&self, id: Uuid) -> MongoResult<Option<PlaylistEntity>> {
        let collection = self.playlist_collection().await;

        collection
            .find_one(doc_id(id))
            .await
            .map_err(|source| MongoDaoError::LoadPlaylist { id, source })
    }

    async fn list_games(&self) -> MongoResult<Vec<GameListItemEntity>> {
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
                entity.into()
            })
            .collect())
    }

    async fn list_playlists(&self) -> MongoResult<Vec<(Uuid, String)>> {
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
}

impl GameStore for MongoGameStore {
    fn save_game(&self, game: GameEntity) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move { store.save_game(game).await.map_err(Into::into) })
    }

    fn save_game_without_teams(&self, game: GameEntity) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move {
            store
                .save_game_without_teams(game)
                .await
                .map_err(Into::into)
        })
    }

    fn save_playlist(&self, playlist: PlaylistEntity) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move { store.save_playlist(playlist).await.map_err(Into::into) })
    }

    fn find_game(&self, id: Uuid) -> BoxFuture<'static, StorageResult<Option<GameEntity>>> {
        let store = self.clone();
        Box::pin(async move { store.find_game(id).await.map_err(Into::into) })
    }

    fn find_playlist(&self, id: Uuid) -> BoxFuture<'static, StorageResult<Option<PlaylistEntity>>> {
        let store = self.clone();
        Box::pin(async move { store.find_playlist(id).await.map_err(Into::into) })
    }

    fn list_games(&self) -> BoxFuture<'static, StorageResult<Vec<GameListItemEntity>>> {
        let store = self.clone();
        Box::pin(async move { store.list_games().await.map_err(Into::into) })
    }

    fn list_playlists(&self) -> BoxFuture<'static, StorageResult<Vec<(Uuid, String)>>> {
        let store = self.clone();
        Box::pin(async move { store.list_playlists().await.map_err(Into::into) })
    }

    fn delete_game(&self, id: Uuid) -> BoxFuture<'static, StorageResult<bool>> {
        let store = self.clone();
        Box::pin(async move { store.delete_game(id).await.map_err(Into::into) })
    }

    fn health_check(&self) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move { store.inner.ping().await.map_err(Into::into) })
    }

    fn try_reconnect(&self) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move { store.inner.reconnect().await.map_err(Into::into) })
    }

    fn save_team(&self, game_id: Uuid, team: TeamEntity) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move {
            // Persist the single team document into the `teams` collection.
            // This keeps per-team updates isolated and avoids touching the game document.
            let team_coll = store.team_collection().await;
            let team_doc: MongoTeamDocument = (game_id, team).into();

            team_coll
                .replace_one(
                    doc! { "game_id": uuid_as_binary(team_doc.game_id), "team_id": uuid_as_binary(team_doc.team_id) },
                    &team_doc,
                )
                .upsert(true)
                .await
                .map_err(|source| MongoDaoError::SaveGame { id: game_id, source })?;

            Ok(())
        })
    }

    fn delete_team(&self, game_id: Uuid, team_id: Uuid) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move {
            let team_coll = store.team_collection().await;
            team_coll
                .delete_one(
                    doc! { "game_id": uuid_as_binary(game_id), "team_id": uuid_as_binary(team_id) },
                )
                .await
                .map_err(|source| MongoDaoError::SaveGame {
                    id: game_id,
                    source,
                })?;

            Ok(())
        })
    }
}
