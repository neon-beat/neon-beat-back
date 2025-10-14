use std::{io, sync::Arc};

use futures::future::BoxFuture;
use reqwest::{Client, Method, StatusCode};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::from_value;
use uuid::Uuid;

use crate::dao::{
    game_store::GameStore,
    models::{GameEntity, PlaylistEntity},
    storage::{StorageError, StorageResult},
};

use super::{
    config::CouchConfig,
    models::{
        AllDocsResponse, CouchGameDocument, CouchPlaylistDocument, END_SUFFIX, GAME_PREFIX,
        PLAYLIST_PREFIX, game_doc_id, playlist_doc_id,
    },
};

#[allow(dead_code)]
#[derive(Clone)]
pub struct CouchGameStore {
    client: Client,
    base_url: Arc<str>,
    database: Arc<str>,
    auth: Option<(Arc<str>, Arc<str>)>,
}

#[allow(dead_code)]
impl CouchGameStore {
    /// Establish a connection to CouchDB and ensure the database exists.
    pub async fn connect(config: CouchConfig) -> StorageResult<Self> {
        let client = Client::builder().build().map_err(|err| {
            StorageError::unavailable("failed to build CouchDB client".into(), err)
        })?;

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

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}/{}/{}", self.base_url, self.database, path);
        let builder = self.client.request(method, url);
        if let Some((ref user, ref pass)) = self.auth {
            builder.basic_auth(user.as_ref(), Some(pass.as_ref()))
        } else {
            builder
        }
    }

    async fn ensure_database(&self) -> StorageResult<()> {
        let url = format!("{}/{}", self.base_url, self.database);
        let mut builder = self.client.get(&url);
        if let Some((ref user, ref pass)) = self.auth {
            builder = builder.basic_auth(user.as_ref(), Some(pass.as_ref()));
        }

        let response = builder.send().await.map_err(|err| {
            StorageError::unavailable("failed to query CouchDB database".into(), err)
        })?;

        match response.status() {
            StatusCode::OK => Ok(()),
            StatusCode::NOT_FOUND => {
                let mut builder = self.client.put(&url);
                if let Some((ref user, ref pass)) = self.auth {
                    builder = builder.basic_auth(user.as_ref(), Some(pass.as_ref()));
                }
                let create = builder.send().await.map_err(|err| {
                    StorageError::unavailable("failed to create CouchDB database".into(), err)
                })?;
                if create.status().is_success() {
                    Ok(())
                } else {
                    let msg = format!(
                        "CouchDB rejected database creation with status {}",
                        create.status()
                    );
                    Err(StorageError::unavailable(
                        msg.clone(),
                        io::Error::new(io::ErrorKind::Other, msg),
                    ))
                }
            }
            other => {
                let msg = format!("unexpected CouchDB database response status {}", other);
                Err(StorageError::unavailable(
                    msg.clone(),
                    io::Error::new(io::ErrorKind::Other, msg),
                ))
            }
        }
    }

    fn couch_error(
        message: impl Into<String>,
        err: impl std::error::Error + Send + Sync + 'static,
    ) -> StorageError {
        StorageError::unavailable(message.into(), err)
    }

    async fn get_document<T>(&self, doc_id: &str) -> StorageResult<Option<T>>
    where
        T: DeserializeOwned,
    {
        let response = self
            .request(Method::GET, doc_id)
            .send()
            .await
            .map_err(|err| Self::couch_error("failed to query CouchDB document", err))?;

        match response.status() {
            StatusCode::NOT_FOUND => Ok(None),
            status if status.is_success() => response
                .json::<T>()
                .await
                .map(Some)
                .map_err(|err| Self::couch_error("failed to deserialize CouchDB document", err)),
            other => {
                let msg = format!(
                    "CouchDB responded with status {} for document {}",
                    other, doc_id
                );
                Err(Self::couch_error(
                    msg.clone(),
                    io::Error::new(io::ErrorKind::Other, msg),
                ))
            }
        }
    }

    async fn put_document<T>(&self, doc_id: &str, document: &T) -> StorageResult<()>
    where
        T: ?Sized + Serialize,
    {
        let response = self
            .request(Method::PUT, doc_id)
            .json(document)
            .send()
            .await
            .map_err(|err| Self::couch_error("failed to send CouchDB PUT", err))?;

        if response.status().is_success() {
            Ok(())
        } else {
            let msg = format!(
                "CouchDB rejected PUT for {} with status {}",
                doc_id,
                response.status()
            );
            Err(Self::couch_error(
                msg.clone(),
                io::Error::new(io::ErrorKind::Other, msg),
            ))
        }
    }

    async fn list_documents<T>(&self, prefix: &str) -> StorageResult<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let query = [
            ("include_docs", "true".to_string()),
            ("startkey", format!("\"{}\"", prefix)),
            ("endkey", format!("\"{}{}\"", prefix, END_SUFFIX)),
        ];

        let response = self
            .request(Method::GET, "_all_docs")
            .query(&query)
            .send()
            .await
            .map_err(|err| Self::couch_error("failed to list CouchDB documents", err))?;

        if !response.status().is_success() {
            let msg = format!(
                "CouchDB rejected _all_docs request with status {}",
                response.status()
            );
            return Err(Self::couch_error(
                msg.clone(),
                io::Error::new(io::ErrorKind::Other, msg),
            ));
        }

        let payload = response
            .json::<AllDocsResponse>()
            .await
            .map_err(|err| Self::couch_error("failed to decode CouchDB _all_docs response", err))?;

        let mut documents = Vec::new();
        for row in payload.rows {
            if let Some(doc) = row.doc {
                let parsed = from_value(doc)
                    .map_err(|err| Self::couch_error("failed to deserialize CouchDB row", err))?;
                documents.push(parsed);
            }
        }

        Ok(documents)
    }
}

impl GameStore for CouchGameStore {
    fn save_game(&self, game: GameEntity) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move {
            let doc_id = game_doc_id(game.id);
            let mut doc = CouchGameDocument::from_entity(game);
            if let Some(existing) = store.get_document::<CouchGameDocument>(&doc_id).await? {
                doc.rev = existing.rev;
            }
            store.put_document(&doc_id, &doc).await
        })
    }

    fn save_playlist(&self, playlist: PlaylistEntity) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move {
            let doc_id = playlist_doc_id(playlist.id);
            let mut doc = CouchPlaylistDocument::from_entity(playlist);
            if let Some(existing) = store.get_document::<CouchPlaylistDocument>(&doc_id).await? {
                doc.rev = existing.rev;
            }
            store.put_document(&doc_id, &doc).await
        })
    }

    fn find_game(&self, id: Uuid) -> BoxFuture<'static, StorageResult<Option<GameEntity>>> {
        let store = self.clone();
        Box::pin(async move {
            let doc_id = game_doc_id(id);
            match store.get_document::<CouchGameDocument>(&doc_id).await? {
                Some(doc) => Ok(Some(doc.into_entity())),
                None => Ok(None),
            }
        })
    }

    fn find_playlist(&self, id: Uuid) -> BoxFuture<'static, StorageResult<Option<PlaylistEntity>>> {
        let store = self.clone();
        Box::pin(async move {
            let doc_id = playlist_doc_id(id);
            match store.get_document::<CouchPlaylistDocument>(&doc_id).await? {
                Some(doc) => Ok(Some(doc.into_entity())),
                None => Ok(None),
            }
        })
    }

    fn list_games(&self) -> BoxFuture<'static, StorageResult<Vec<(Uuid, String)>>> {
        let store = self.clone();
        Box::pin(async move {
            let docs = store
                .list_documents::<CouchGameDocument>(GAME_PREFIX)
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
                .map_err(|err| StorageError::unavailable("failed to ping CouchDB".into(), err))?;
            if response.status().is_success() {
                Ok(())
            } else {
                let msg = format!("CouchDB ping failed with status {}", response.status());
                Err(StorageError::unavailable(
                    msg.clone(),
                    io::Error::new(io::ErrorKind::Other, msg),
                ))
            }
        })
    }

    fn try_reconnect(&self) -> BoxFuture<'static, StorageResult<()>> {
        let store = self.clone();
        Box::pin(async move { store.ensure_database().await })
    }
}
