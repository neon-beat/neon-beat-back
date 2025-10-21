pub mod game;
mod sse;
pub mod state_machine;
pub mod transitions;

use std::{sync::Arc, time::Duration};

use crate::services::websocket_service::send_message_to_websocket;
use crate::{
    dao::game_store::GameStore,
    dto::ws::BuzzFeedback,
    error::ServiceError,
    state::{
        game::{GameSession, Team},
        state_machine::{GamePhase, PairingSession},
    },
};
use axum::extract::ws::Message;
use dashmap::DashMap;
use indexmap::IndexMap;
use tokio::sync::{Mutex, RwLock, mpsc, watch};
use tokio::time::timeout;
use tracing::warn;
use uuid::Uuid;

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
    game_store: RwLock<Option<Arc<dyn GameStore>>>,
    sse: SseState,
    buzzers: DashMap<String, BuzzerConnection>,
    game: RwLock<GameStateMachine>,
    current_game: RwLock<Option<GameSession>>,
    degraded_flag: RwLock<bool>,
    degraded_tx: watch::Sender<bool>,
    transition_gate: Mutex<()>,
    transition_timeout: Option<Duration>,
}

impl AppState {
    /// Construct a new [`AppState`] wrapped in an [`Arc`] so it can be cloned cheaply.
    ///
    /// The application starts in degraded mode until a storage backend is installed.
    pub fn new() -> SharedState {
        let (degraded_tx, _rx) = watch::channel(true);
        Arc::new(Self {
            game_store: RwLock::new(None),
            sse: SseState::new(16, 16),
            buzzers: DashMap::new(),
            game: RwLock::new(GameStateMachine::new()),
            current_game: RwLock::new(None),
            degraded_flag: RwLock::new(true),
            degraded_tx,
            transition_gate: Mutex::new(()),
            transition_timeout: Some(DEFAULT_TRANSITION_TIMEOUT),
        })
    }

    /// Obtain a handle to the current game store, if one is installed.
    pub async fn game_store(&self) -> Option<Arc<dyn GameStore>> {
        let guard = self.game_store.read().await;
        guard.as_ref().cloned()
    }

    /// Retrieve the configured game store or report degraded mode.
    pub async fn require_game_store(&self) -> Result<Arc<dyn GameStore>, ServiceError> {
        self.game_store().await.ok_or(ServiceError::Degraded)
    }

    /// Persist the current in-memory game back into the configured store.
    pub async fn persist_current_game(&self) -> Result<(), ServiceError> {
        let store = self.require_game_store().await?;
        let snapshot = {
            let guard = self.current_game.read().await;
            guard
                .as_ref()
                .cloned()
                .ok_or_else(|| ServiceError::InvalidState("no active game".into()))?
        };
        store.save_game(snapshot.into()).await?;
        Ok(())
    }

    /// Install a new game store implementation and leave degraded mode.
    pub async fn set_game_store(&self, store: Arc<dyn GameStore>) {
        {
            let mut guard = self.game_store.write().await;
            *guard = Some(store);
        }
        self.update_degraded(false).await;
    }

    /// Current degraded flag.
    pub async fn is_degraded(&self) -> bool {
        *self.degraded_flag.read().await
    }

    /// Subscribe to degraded mode updates.
    pub fn degraded_watcher(&self) -> watch::Receiver<bool> {
        self.degraded_tx.subscribe()
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

    /// Snapshot the current pairing session if one is active.
    pub async fn pairing_session(&self) -> Option<PairingSession> {
        let sm = self.game.read().await;
        sm.pairing_session().cloned()
    }

    /// Mutate the active pairing session, if any, returning the closure result.
    ///
    /// Callers may return domain-specific errors from the closure; if the session is not currently
    /// active a `ServiceError::InvalidState` is returned instead.
    pub async fn with_pairing_session_mut<F, T>(&self, f: F) -> Result<T, ServiceError>
    where
        F: FnOnce(&mut PairingSession) -> T,
    {
        let mut sm = self.game.write().await;
        match sm.pairing_session_mut() {
            Some(session) => Ok(f(session)),
            None => Err(ServiceError::InvalidState(
                "pairing session is not active".into(),
            )),
        }
    }

    /// Check whether every team in `teams` has an active buzzer connection registered.
    pub fn all_teams_paired(&self, teams: &IndexMap<Uuid, Team>) -> bool {
        teams.iter().all(|(_, team)| {
            team.buzzer_id
                .as_ref()
                .is_some_and(|id| self.buzzers.contains_key(id))
        })
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
    pub async fn update_degraded(&self, value: bool) {
        {
            let mut guard = self.degraded_flag.write().await;
            if *guard == value {
                return;
            }
            *guard = value;
        }

        let _ = self.degraded_tx.send(value);
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
