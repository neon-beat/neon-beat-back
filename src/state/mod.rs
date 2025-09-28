mod sse;

use std::sync::Arc;

use axum::extract::ws::Message;
use dashmap::DashMap;
use mongodb::Database;
use tokio::sync::{Mutex, mpsc};

use crate::dao::mongodb::MongoManager;

use self::sse::SseState;

pub use self::sse::SseHub;

pub type SharedState = Arc<AppState>;

#[derive(Clone)]
/// Handle used to push messages to a connected buzzer.
pub struct BuzzerConnection {
    pub id: String,
    pub tx: mpsc::UnboundedSender<Message>,
}

/// Central application state storing persistent connections and database handles.
pub struct AppState {
    mongo: MongoManager,
    sse: SseState,
    buzzers: DashMap<String, BuzzerConnection>,
}

impl AppState {
    /// Construct a new [`AppState`] wrapped in an [`Arc`] so it can be cloned cheaply.
    pub fn new(mongo: MongoManager) -> SharedState {
        Arc::new(Self {
            mongo,
            sse: SseState::new(16, 16),
            buzzers: DashMap::new(),
        })
    }

    /// Clone the MongoDB database handle for DAO layers.
    pub async fn database(&self) -> Database {
        self.mongo.database().await
    }

    /// Accessor for the MongoDB manager.
    pub fn mongo(&self) -> MongoManager {
        self.mongo.clone()
    }

    /// Broadcast hub used for the public SSE stream.
    pub fn public_sse(&self) -> &SseHub {
        self.sse.public()
    }

    /// Broadcast hub used for the admin SSE stream.
    pub fn admin_sse(&self) -> &SseHub {
        self.sse.admin().hub()
    }

    /// Token guard that ensures a single admin SSE subscriber at a time.
    pub fn admin_token(&self) -> &Mutex<Option<String>> {
        self.sse.admin().token()
    }

    /// Registry of active buzzer sockets keyed by their identifier.
    pub fn buzzers(&self) -> &DashMap<String, BuzzerConnection> {
        &self.buzzers
    }
}
