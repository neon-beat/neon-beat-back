use std::{io, sync::Arc};

use futures::future::BoxFuture;
use reqwest::{Client, Method, StatusCode};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Error as JsonError, from_value};
use uuid::Uuid;

use crate::dao::{
    game_store::GameStore,
    models::{GameEntity, GameListItemEntity, PlaylistEntity},
    storage::{StorageError, StorageResult},
};

use super::{
    config::CouchConfig,
    error::{CouchDaoError, CouchResult},
    models::{
        AllDocsResponse, CouchGameDocument, CouchPlaylistDocument, END_SUFFIX, GAME_PREFIX,
        PLAYLIST_PREFIX, game_doc_id, playlist_doc_id,
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
}

impl GameStore for CouchGameStore {
    /// Persist a [`GameEntity`] into CouchDB, preserving revisions.
    fn save_game(&self, game: GameEntity) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move {
            let doc_id = game_doc_id(game.id);
            let mut doc = CouchGameDocument::from_entity(game);
            if let Some(existing) = store.get_document::<CouchGameDocument>(&doc_id).await? {
                doc.rev = existing.rev;
            }
            store.put_document(&doc_id, &doc).await.map_err(Into::into)
        })
    }

    /// Persist a [`PlaylistEntity`] into CouchDB.
    fn save_playlist(&self, playlist: PlaylistEntity) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move {
            let doc_id = playlist_doc_id(playlist.id);
            let mut doc = CouchPlaylistDocument::from_entity(playlist);
            if let Some(existing) = store.get_document::<CouchPlaylistDocument>(&doc_id).await? {
                doc.rev = existing.rev;
            }
            store.put_document(&doc_id, &doc).await.map_err(Into::into)
        })
    }

    /// Load a single [`GameEntity`] from CouchDB.
    fn find_game(&self, id: Uuid) -> BoxFuture<'static, StorageResult<Option<GameEntity>>> {
        let store = self.clone();
        Box::pin(async move {
            let doc_id = game_doc_id(id);
            let maybe_doc = store.get_document::<CouchGameDocument>(&doc_id).await?;
            Ok(maybe_doc.map(|doc| doc.into_entity()))
        })
    }

    /// Load a single [`PlaylistEntity`] from CouchDB.
    fn find_playlist(&self, id: Uuid) -> BoxFuture<'static, StorageResult<Option<PlaylistEntity>>> {
        let store = self.clone();
        Box::pin(async move {
            let doc_id = playlist_doc_id(id);
            let maybe_doc = store.get_document::<CouchPlaylistDocument>(&doc_id).await?;
            Ok(maybe_doc.map(|doc| doc.into_entity()))
        })
    }

    /// Produce a list of known games comprising identifiers and titles.
    fn list_games(&self) -> BoxFuture<'static, StorageResult<Vec<GameListItemEntity>>> {
        let store = self.clone();
        Box::pin(async move {
            let docs = store
                .list_documents::<CouchGameDocument>(GAME_PREFIX)
                .await?;
            Ok(docs
                .into_iter()
                .map(|doc| doc.into_entity().into())
                .collect())
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
                .map(|doc| {
                    let entity = doc.into_entity();
                    (entity.id, entity.name)
                })
                .collect())
        })
    }

    fn delete_game(&self, id: Uuid) -> BoxFuture<'static, StorageResult<bool>> {
        let store = self.clone();
        Box::pin(async move {
            let doc_id = game_doc_id(id);
            let Some(doc) = store.get_document::<CouchGameDocument>(&doc_id).await? else {
                return Ok(false);
            };

            let rev = doc.rev.ok_or_else(|| CouchDaoError::DeserializeValue {
                path: doc_id.clone(),
                source: JsonError::io(io::Error::new(
                    io::ErrorKind::Other,
                    "missing _rev for CouchDB document",
                )),
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
