pub mod game;
mod sse;
pub mod state_machine;

use std::sync::Arc;

use axum::extract::ws::Message;
use dashmap::DashMap;
use tokio::sync::{Mutex, RwLock, mpsc, watch};

use crate::{dao::mongodb::MongoManager, state::game::GameSession};

pub use self::sse::SseHub;
use self::{sse::SseState, state_machine::GameStateMachine};

pub type SharedState = Arc<AppState>;

#[derive(Clone)]
/// Handle used to push messages to a connected buzzer.
pub struct BuzzerConnection {
    pub id: String,
    pub tx: mpsc::UnboundedSender<Message>,
}

/// Central application state storing persistent connections and database handles.
pub struct AppState {
    mongo: RwLock<Option<MongoManager>>,
    sse: SseState,
    buzzers: DashMap<String, BuzzerConnection>,
    game: RwLock<GameStateMachine>,
    current_game: RwLock<Option<GameSession>>,
    degraded: watch::Sender<bool>,
}

impl AppState {
    /// Construct a new [`AppState`] wrapped in an [`Arc`] so it can be cloned cheaply.
    ///
    /// The application starts in degraded mode until the MongoDB supervisor
    /// establishes a connection and installs a [`MongoManager`].
    pub fn new() -> SharedState {
        let (degraded_tx, _rx) = watch::channel(true);
        Arc::new(Self {
            mongo: RwLock::new(None),
            sse: SseState::new(16, 16),
            buzzers: DashMap::new(),
            game: RwLock::new(GameStateMachine::new()),
            current_game: RwLock::new(None),
            degraded: degraded_tx,
        })
    }

    /// Obtain a cloned MongoDB manager, if a connection is currently available.
    pub async fn mongo(&self) -> Option<MongoManager> {
        let guard = self.mongo.read().await;
        guard.clone()
    }

    /// Replace the MongoDB manager and leave degraded mode.
    pub async fn install_mongo(&self, manager: MongoManager) {
        {
            let mut guard = self.mongo.write().await;
            *guard = Some(manager);
        }
        self.update_degraded(false).await;
    }

    /// Remove the MongoDB manager and enter degraded mode.
    pub async fn clear_mongo(&self) {
        {
            let mut guard = self.mongo.write().await;
            guard.take();
        }
        self.update_degraded(true).await;
    }

    /// Current degraded flag.
    pub async fn is_degraded(&self) -> bool {
        let guard = self.mongo.read().await;
        guard.is_none()
    }

    /// Subscribe to degraded mode updates.
    pub fn degraded_watcher(&self) -> watch::Receiver<bool> {
        self.degraded.subscribe()
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

    /// Shared game state machine guarding gameplay transitions.
    pub fn game(&self) -> &RwLock<GameStateMachine> {
        &self.game
    }

    /// Currently active game session data.
    pub fn current_game(&self) -> &RwLock<Option<GameSession>> {
        &self.current_game
    }

    /// Update and broadcast the degraded flag when the value changes.
    async fn update_degraded(&self, value: bool) {
        if self.is_degraded().await == value {
            return;
        }

        let _ = self.degraded.send(value);
    }
}
