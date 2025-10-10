pub mod game;
mod sse;
pub mod state_machine;
pub mod transitions;

use std::{sync::Arc, time::Duration};

use axum::extract::ws::Message;
use dashmap::DashMap;
use tokio::sync::{Mutex, RwLock, mpsc, watch};
use tokio::time::timeout;
use tracing::warn;

use crate::services::websocket_service::send_message_to_websocket;
use crate::{
    dao::mongodb::MongoManager,
    dto::ws::BuzzFeedback,
    error::ServiceError,
    state::{game::GameSession, state_machine::GamePhase},
};

pub use self::sse::SseHub;
pub use self::state_machine::{AbortError, ApplyError, Plan, PlanError, PlanId, Snapshot};
use self::{
    sse::SseState,
    state_machine::{GameEvent, GameStateMachine},
};

pub type SharedState = Arc<AppState>;
pub const DEFAULT_TRANSITION_TIMEOUT: Duration = Duration::from_secs(5);

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
    transition_gate: Mutex<()>,
    transition_timeout: Option<Duration>,
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
            transition_gate: Mutex::new(()),
            transition_timeout: Some(DEFAULT_TRANSITION_TIMEOUT),
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

    /// Snapshot the current phase of the shared game state machine.
    pub async fn state_machine_phase(&self) -> GamePhase {
        self.game.read().await.phase()
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

    /// Plan a transition to the shared game state machine, returning the plan.
    async fn plan_transition(&self, event: GameEvent) -> Result<Plan, PlanError> {
        let mut sm = self.game.write().await;
        sm.plan(event)
    }

    /// Apply the planned transition to the shared game state machine, returning the next phase.
    async fn apply_planned_transition(&self, plan_id: PlanId) -> Result<GamePhase, ApplyError> {
        let mut sm = self.game.write().await;
        sm.apply(plan_id)
    }

    /// Abort a planned transition of the shared game state machine
    async fn abort_transition(&self, plan_id: PlanId) -> Result<(), AbortError> {
        let mut sm = self.game.write().await;
        sm.abort(plan_id)
    }

    pub async fn snapshot(&self) -> Snapshot {
        let sm = self.game.read().await;
        sm.snapshot()
    }

    pub async fn run_transition<F, Fut, T>(
        &self,
        event: GameEvent,
        work: F,
    ) -> Result<(T, GamePhase), ServiceError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, ServiceError>>,
    {
        let gate = self.transition_gate.lock().await;
        let Plan { id: plan_id, .. } = self.plan_transition(event.clone()).await?;

        let work_future = work();
        let outcome = if let Some(limit) = self.transition_timeout {
            match timeout(limit, work_future).await {
                Ok(result) => result,
                Err(_) => {
                    if let Err(abort_err) = self.abort_transition(plan_id).await {
                        warn!(
                            event = ?event,
                            plan_id = %plan_id,
                            error = ?abort_err,
                            "failed to abort transition after timeout"
                        );
                    }
                    drop(gate);
                    return Err(ServiceError::Timeout);
                }
            }
        } else {
            work_future.await
        };

        match outcome {
            Ok(value) => {
                let next = self.apply_planned_transition(plan_id).await?;
                drop(gate);
                Ok((value, next))
            }
            Err(err) => {
                if let Err(abort_err) = self.abort_transition(plan_id).await {
                    warn!(
                        event = ?event,
                        plan_id = %plan_id,
                        error = ?abort_err,
                        "failed to abort transition after work error"
                    );
                }
                drop(gate);
                Err(err)
            }
        }
    }

    pub fn notify_buzzer_turn_finished(&self, buzzer_id: &str) {
        let Some(connection) = self.buzzers.get(buzzer_id) else {
            return;
        };

        let tx = connection.tx.clone();
        drop(connection);

        send_message_to_websocket(
            &tx,
            &BuzzFeedback {
                id: buzzer_id.into(),
                can_answer: false,
            },
            "buzzer turn ended",
        );
    }
}
