pub mod game;
mod sse;
pub mod state_machine;

use std::sync::Arc;

use axum::extract::ws::Message;
use dashmap::DashMap;
use mongodb::Database;
use tokio::sync::{Mutex, RwLock, mpsc};

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
    mongo: MongoManager,
    sse: SseState,
    buzzers: DashMap<String, BuzzerConnection>,
    game: RwLock<GameStateMachine>,
    current_game: RwLock<Option<GameSession>>,
}

impl AppState {
    /// Construct a new [`AppState`] wrapped in an [`Arc`] so it can be cloned cheaply.
    pub fn new(mongo: MongoManager) -> SharedState {
        Arc::new(Self {
            mongo,
            sse: SseState::new(16, 16),
            buzzers: DashMap::new(),
            game: RwLock::new(GameStateMachine::new()),
            current_game: RwLock::new(None),
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

    /// Shared game state machine guarding gameplay transitions.
    pub fn game(&self) -> &RwLock<GameStateMachine> {
        &self.game
    }

    /// Currently active game session data.
    pub fn current_game(&self) -> &RwLock<Option<GameSession>> {
        &self.current_game
    }
}
