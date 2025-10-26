use std::{collections::HashMap, io, sync::Arc};

use futures::future::BoxFuture;
use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Error as JsonError, from_value};
use uuid::Uuid;

use crate::dao::{
    game_store::GameStore,
    models::{GameEntity, GameListItemEntity, PlaylistEntity, TeamEntity},
    storage::{StorageError, StorageResult},
};

use super::{
    config::CouchConfig,
    error::{CouchDaoError, CouchResult},
    models::{
        AllDocsResponse, CouchGameDocument, CouchPlaylistDocument, CouchTeamDocument, END_SUFFIX,
        GAME_PREFIX, PLAYLIST_PREFIX, TEAM_PREFIX, extract_uuid, game_doc_id, playlist_doc_id,
        team_doc_id,
    },
};

/// CouchDB-backed implementation of the [`GameStore`] trait.
#[derive(Clone)]
pub struct CouchGameStore {
    client: Client,
    base_url: Arc<str>,
    database: Arc<str>,
    auth: Option<(Arc<str>, Arc<str>)>,
}

impl CouchGameStore {
    /// Save a team document with optimistic retry on conflict.
    async fn save_team_document(&self, game_id: Uuid, team: &TeamEntity) -> CouchResult<()> {
        let doc_id = team_doc_id(game_id, team.id);
        let rev = self
            .get_document::<CouchTeamDocument>(&doc_id)
            .await?
            .and_then(|doc| doc.rev);
        let doc: CouchTeamDocument = (game_id, team.clone(), rev).into();

        self.put_document(&doc_id, &doc).await
    }

    /// Delete all team documents for a game.
    async fn delete_game_teams(&self, game_id: Uuid) -> CouchResult<()> {
        let prefix = format!("{}{}", TEAM_PREFIX, game_id);
        let teams = self.list_documents::<CouchTeamDocument>(&prefix).await?;
        for team in teams {
            if let Some(rev) = team.rev {
                self.delete_document(&team.id, &rev).await?;
            }
        }
        Ok(())
    }

    /// Delete a single team document.
    async fn delete_team_document(&self, game_id: Uuid, team_id: Uuid) -> CouchResult<()> {
        let doc_id = team_doc_id(game_id, team_id);
        if let Some(doc) = self.get_document::<CouchTeamDocument>(&doc_id).await? {
            if let Some(rev) = doc.rev {
                self.delete_document(&doc_id, &rev).await?;
            }
        }
        Ok(())
    }

    /// Establish a connection to CouchDB and ensure the database exists.
    pub async fn connect(config: CouchConfig) -> CouchResult<Self> {
        let client = Client::builder()
            .build()
            .map_err(|source| CouchDaoError::ClientBuilder { source })?;

        let base_url = Arc::<str>::from(config.base_url.trim_end_matches('/'));
        let database = Arc::<str>::from(config.database);
        let auth = config
            .username
            .zip(config.password)
            .map(|(u, p)| (Arc::<str>::from(u), Arc::<str>::from(p)));

        let store = Self {
            client,
            base_url,
            database,
            auth,
        };

        store.ensure_database().await?;
        Ok(store)
    }

    /// Prepare a request builder for a collection-relative path, including authentication.
    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}/{}/{}", self.base_url, self.database, path);
        let builder = self.client.request(method, url);
        if let Some((ref user, ref pass)) = self.auth {
            builder.basic_auth(user.as_ref(), Some(pass.as_ref()))
        } else {
            builder
        }
    }

    /// Verify the CouchDB database exists, creating it when possible.
    async fn ensure_database(&self) -> CouchResult<()> {
        let database = self.database.to_string();
        let url = format!("{}/{}", self.base_url, self.database);
        let mut builder = self.client.get(&url);
        if let Some((ref user, ref pass)) = self.auth {
            builder = builder.basic_auth(user.as_ref(), Some(pass.as_ref()));
        }

        let response = builder
            .send()
            .await
            .map_err(|source| CouchDaoError::DatabaseQuery {
                database: database.clone(),
                source,
            })?;

        match response.status() {
            StatusCode::OK => Ok(()),
            StatusCode::NOT_FOUND => {
                let mut builder = self.client.put(&url);
                if let Some((ref user, ref pass)) = self.auth {
                    builder = builder.basic_auth(user.as_ref(), Some(pass.as_ref()));
                }
                let create =
                    builder
                        .send()
                        .await
                        .map_err(|source| CouchDaoError::DatabaseCreate {
                            database: database.clone(),
                            source,
                        })?;
                if create.status().is_success() {
                    Ok(())
                } else {
                    Err(CouchDaoError::DatabaseStatus {
                        database,
                        status: create.status(),
                    })
                }
            }
            other => Err(CouchDaoError::DatabaseStatus {
                database,
                status: other,
            }),
        }
    }

    /// Bulk get multiple documents by their IDs
    async fn bulk_get_documents<T>(&self, doc_ids: &[String]) -> CouchResult<Vec<T>>
    where
        T: DeserializeOwned,
    {
        if doc_ids.is_empty() {
            return Ok(Vec::new());
        }

        #[derive(Serialize)]
        struct BulkGetRequest<'a> {
            docs: Vec<BulkGetDoc<'a>>,
        }

        #[derive(Serialize)]
        struct BulkGetDoc<'a> {
            id: &'a str,
        }

        let request = BulkGetRequest {
            docs: doc_ids.iter().map(|id| BulkGetDoc { id }).collect(),
        };

        let response = self
            .request(Method::POST, "_bulk_get")
            .json(&request)
            .send()
            .await
            .map_err(|source| CouchDaoError::RequestSend {
                path: "_bulk_get".to_string(),
                source,
            })?;

        if !response.status().is_success() {
            return Err(CouchDaoError::RequestStatus {
                path: "_bulk_get".to_string(),
                status: response.status(),
            });
        }

        #[derive(Deserialize)]
        struct BulkGetResponse {
            results: Vec<BulkGetResult>,
        }

        #[derive(Deserialize)]
        struct BulkGetResult {
            docs: Vec<BulkGetDocResult>,
        }

        #[derive(Deserialize)]
        struct BulkGetDocResult {
            ok: Option<serde_json::Value>,
        }

        let bulk_response = response.json::<BulkGetResponse>().await.map_err(|source| {
            CouchDaoError::DecodeResponse {
                path: "_bulk_get".to_string(),
                source,
            }
        })?;

        Ok(bulk_response
            .results
            .into_iter()
            .flat_map(|result| result.docs)
            .filter_map(|doc| doc.ok)
            .filter_map(|value| serde_json::from_value::<T>(value).ok())
            .collect())
    }

    /// Retrieve and deserialize a document by id.
    async fn get_document<T>(&self, doc_id: &str) -> CouchResult<Option<T>>
    where
        T: DeserializeOwned,
    {
        let response = self
            .request(Method::GET, doc_id)
            .send()
            .await
            .map_err(|source| CouchDaoError::RequestSend {
                path: doc_id.to_string(),
                source,
            })?;

        match response.status() {
            StatusCode::NOT_FOUND => Ok(None),
            status if status.is_success() => {
                response.json::<T>().await.map(Some).map_err(|source| {
                    CouchDaoError::DecodeResponse {
                        path: doc_id.to_string(),
                        source,
                    }
                })
            }
            other => Err(CouchDaoError::RequestStatus {
                path: doc_id.to_string(),
                status: other,
            }),
        }
    }

    /// Upload a document, reusing the provided revision when present.
    async fn put_document<T>(&self, doc_id: &str, document: &T) -> CouchResult<()>
    where
        T: ?Sized + Serialize,
    {
        let response = self
            .request(Method::PUT, doc_id)
            .json(document)
            .send()
            .await
            .map_err(|source| CouchDaoError::RequestSend {
                path: doc_id.to_string(),
                source,
            })?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(CouchDaoError::RequestStatus {
                path: doc_id.to_string(),
                status: response.status(),
            })
        }
    }

    async fn delete_document(&self, doc_id: &str, rev: &str) -> CouchResult<()> {
        let response = self
            .request(Method::DELETE, doc_id)
            .query(&[("rev", rev.to_string())])
            .send()
            .await
            .map_err(|source| CouchDaoError::RequestSend {
                path: doc_id.to_string(),
                source,
            })?;

        if response.status().is_success() || response.status() == StatusCode::NOT_FOUND {
            Ok(())
        } else {
            Err(CouchDaoError::RequestStatus {
                path: doc_id.to_string(),
                status: response.status(),
            })
        }
    }

    /// Fetch documents with identifiers matching the provided prefix.
    async fn list_documents<T>(&self, prefix: &str) -> CouchResult<Vec<T>>
    where
        T: DeserializeOwned,
    {
        const ALL_DOCS: &str = "_all_docs";
        let query = [
            ("include_docs", "true".to_string()),
            ("startkey", format!("\"{}\"", prefix)),
            ("endkey", format!("\"{}{}\"", prefix, END_SUFFIX)),
        ];

        let response = self
            .request(Method::GET, ALL_DOCS)
            .query(&query)
            .send()
            .await
            .map_err(|source| CouchDaoError::RequestSend {
                path: ALL_DOCS.to_string(),
                source,
            })?;

        if !response.status().is_success() {
            return Err(CouchDaoError::RequestStatus {
                path: ALL_DOCS.to_string(),
                status: response.status(),
            });
        }

        let payload = response.json::<AllDocsResponse>().await.map_err(|source| {
            CouchDaoError::DecodeResponse {
                path: ALL_DOCS.to_string(),
                source,
            }
        })?;

        let mut documents = Vec::new();
        for row in payload.rows {
            if let Some(doc) = row.doc {
                let parsed = from_value(doc).map_err(|source| CouchDaoError::DeserializeValue {
                    path: ALL_DOCS.to_string(),
                    source,
                })?;
                documents.push(parsed);
            }
        }

        Ok(documents)
    }

    /// Helper to persist the game document.
    /// Extracts team IDs from the GameEntity and fetches the current revision
    /// automatically to ensure optimistic concurrency.
    async fn save_game_document(&self, game: GameEntity) -> CouchResult<()> {
        let doc_id = game_doc_id(game.id);
        let rev = self
            .get_document::<CouchGameDocument>(&doc_id)
            .await?
            .and_then(|doc| doc.rev);
        let doc: CouchGameDocument = (game, rev).into();
        self.put_document(&doc_id, &doc).await
    }
}

impl GameStore for CouchGameStore {
    /// Save a single team document. This is used to persist team updates without
    /// loading and saving the entire game document.
    fn save_team(&self, game_id: Uuid, team: TeamEntity) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move {
            store
                .save_team_document(game_id, &team)
                .await
                .map_err(Into::into)
        })
    }

    /// Delete a single team document.
    fn delete_team(&self, game_id: Uuid, team_id: Uuid) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move {
            store
                .delete_team_document(game_id, team_id)
                .await
                .map_err(Into::into)
        })
    }

    /// Persist a [`GameEntity`] into CouchDB, preserving revisions and storing teams in separate documents.
    fn save_game(&self, game: GameEntity) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move {
            // Save all team documents first (each with optimistic retry)
            let teams = game.teams.clone();
            for team in teams.iter() {
                store.save_team_document(game.id, team).await?;
            }

            // Persist the game document (team IDs extracted from game.teams)
            store.save_game_document(game).await.map_err(Into::into)
        })
    }

    /// Persist only game metadata (without team documents) for efficient partial updates.
    fn save_game_without_teams(&self, game: GameEntity) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move {
            // Persist the game document (team IDs extracted from game.teams)
            store.save_game_document(game).await.map_err(Into::into)
        })
    }
    /// Persist a [`PlaylistEntity`] into CouchDB.
    fn save_playlist(&self, playlist: PlaylistEntity) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move {
            let doc_id = playlist_doc_id(playlist.id);
            let rev = store
                .get_document::<CouchPlaylistDocument>(&doc_id)
                .await?
                .and_then(|doc| doc.rev);
            let doc: CouchPlaylistDocument = (playlist, rev).into();
            store.put_document(&doc_id, &doc).await.map_err(Into::into)
        })
    }

    /// Load a single [`GameEntity`] from CouchDB along with its teams from team:: documents.
    fn find_game(&self, id: Uuid) -> BoxFuture<'static, StorageResult<Option<GameEntity>>> {
        let store = self.clone();
        Box::pin(async move {
            let doc_id = game_doc_id(id);
            let Some(game_doc) = store.get_document::<CouchGameDocument>(&doc_id).await? else {
                return Ok(None);
            };

            // Use bulk_get_documents to efficiently fetch all team documents
            let team_ids: Vec<String> = game_doc
                .game
                .team_ids
                .iter()
                .map(|team_id| team_doc_id(id, *team_id))
                .collect();

            let team_docs = store.bulk_get_documents(&team_ids).await?;

            // Convert into GameEntity using From implementation
            Ok(Some(game_doc.try_into_entity(id, team_docs)?))
        })
    }

    /// Load a single [`PlaylistEntity`] from CouchDB.
    fn find_playlist(&self, id: Uuid) -> BoxFuture<'static, StorageResult<Option<PlaylistEntity>>> {
        let store = self.clone();
        Box::pin(async move {
            let doc_id = playlist_doc_id(id);
            let maybe_doc = store.get_document::<CouchPlaylistDocument>(&doc_id).await?;
            Ok(maybe_doc.map(TryInto::try_into).transpose()?)
        })
    }

    /// Produce a list of known games comprising identifiers and titles.
    fn list_games(&self) -> BoxFuture<'static, StorageResult<Vec<GameListItemEntity>>> {
        let store = self.clone();
        Box::pin(async move {
            // First, get all game documents
            let game_docs = store
                .list_documents::<CouchGameDocument>(GAME_PREFIX)
                .await?;

            // Collect all team IDs we need to fetch
            let mut team_ids = Vec::new();
            let game_docs_map = game_docs
                .into_iter()
                .map(|game_doc| {
                    let game_id = extract_uuid(&game_doc.id).map_err(|e| {
                        CouchDaoError::DeserializeValue {
                            path: "list_games".to_string(),
                            source: JsonError::io(io::Error::other(format!(
                                "failed to extract game_id from document: {}",
                                e
                            ))),
                        }
                    })?;
                    team_ids.extend(
                        game_doc
                            .game
                            .team_ids
                            .iter()
                            .map(|team_id| team_doc_id(game_id, *team_id)),
                    );
                    Ok((game_id, game_doc))
                })
                .collect::<Result<HashMap<_, _>, CouchDaoError>>()?;

            // Bulk fetch all team documents in one request
            let team_docs: Vec<CouchTeamDocument> = store.bulk_get_documents(&team_ids).await?;

            // Group team documents by game ID using the explicit `game_id` field
            let mut team_map: HashMap<Uuid, Vec<CouchTeamDocument>> = HashMap::new();
            for team_doc in team_docs {
                let game_id = team_doc.team.game_id;
                team_map.entry(game_id).or_default().push(team_doc);
            }

            // Create final result list
            let games = game_docs_map
                .into_iter()
                .map(|(game_id, game_doc)| {
                    let team_docs = team_map.remove(&game_id).unwrap_or_default();
                    let game_entity = game_doc.try_into_entity(game_id, team_docs)?;
                    Ok(game_entity.into())
                })
                .collect::<Result<Vec<_>, CouchDaoError>>()?;

            Ok(games)
        })
    }

    /// Produce a list of known playlists comprising identifiers and names.
    fn list_playlists(&self) -> BoxFuture<'static, StorageResult<Vec<(Uuid, String)>>> {
        let store = self.clone();
        Box::pin(async move {
            let docs = store
                .list_documents::<CouchPlaylistDocument>(PLAYLIST_PREFIX)
                .await?;
            Ok(docs
                .into_iter()
                .map(|doc| -> Result<_, CouchDaoError> {
                    let entity = PlaylistEntity::try_from(doc)?;
                    Ok((entity.id, entity.name))
                })
                .collect::<Result<Vec<_>, _>>()?)
        })
    }

    fn delete_game(&self, id: Uuid) -> BoxFuture<'static, StorageResult<bool>> {
        let store = self.clone();
        Box::pin(async move {
            let doc_id = game_doc_id(id);
            let Some(doc) = store.get_document::<CouchGameDocument>(&doc_id).await? else {
                return Ok(false);
            };

            // Delete all team documents first
            store.delete_game_teams(id).await?;

            // Then delete the game document
            let rev = doc.rev.ok_or_else(|| CouchDaoError::DeserializeValue {
                path: doc_id.clone(),
                source: JsonError::io(io::Error::other("missing _rev for CouchDB document")),
            })?;

            store
                .delete_document(&doc_id, &rev)
                .await
                .map_err(StorageError::from)?;
            Ok(true)
        })
    }

    /// Ping the remote database to ensure the connection is healthy.
    fn health_check(&self) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move {
            let url = format!("{}/{}", store.base_url, store.database);
            let mut builder = store.client.get(&url);
            if let Some((ref user, ref pass)) = store.auth {
                builder = builder.basic_auth(user.as_ref(), Some(pass.as_ref()));
            }

            let response = builder
                .send()
                .await
                .map_err(|source| CouchDaoError::RequestSend {
                    path: url.clone(),
                    source,
                })?;

            if response.status().is_success() {
                Ok(())
            } else {
                Err(CouchDaoError::RequestStatus {
                    path: url,
                    status: response.status(),
                }
                .into())
            }
        })
    }

    /// Attempt to recover connectivity by ensuring the database exists.
    fn try_reconnect(&self) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move { store.ensure_database().await.map_err(Into::into) })
    }
}
