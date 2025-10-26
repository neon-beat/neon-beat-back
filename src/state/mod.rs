pub mod game;
mod sse;
pub mod state_machine;
pub mod transitions;

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    config::{AppConfig, BuzzerPatternPreset},
    dao::{game_store::GameStore, models::TeamEntity},
    dto::{
        common::{GamePhaseSnapshot, SongSnapshot},
        game::TeamSummary,
        phase::VisibleGamePhase,
        ws::BuzzerPattern,
    },
    error::ServiceError,
    state::{
        game::{GameSession, Team},
        state_machine::{GamePhase, GameRunningPhase, PairingSession, PauseKind, PrepStatus},
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

/// Coordinates persistence operations with locking and throttling.
///
/// This component encapsulates all persistence coordination state to prevent:
/// - Concurrent writes causing CouchDB revision conflicts
/// - Rapid-fire updates overwhelming the database
///
/// Uses separate coordination for game-level and team-level persistence to allow
/// different teams to persist concurrently while maintaining serial writes per team.
struct PersistenceCoordinator {
    /// Mutex used to serialize full game persistent saves to avoid concurrent PUTs.
    game_lock: Mutex<()>,
    /// Timestamp of last successful game persist, used for throttling.
    game_last_persist: RwLock<Option<Instant>>,
    /// Per-team persistence metadata (lock + throttle timestamp).
    /// Keyed by team_id only since only one game is active at a time.
    team_metadata: DashMap<Uuid, TeamPersistMetadata>,
}

/// Metadata for coordinating team persistence operations.
/// Encapsulates both the lock (for serialization) and throttle timestamp (for rate limiting).
struct TeamPersistMetadata {
    /// Lock ensuring serial saves of this team document to avoid CouchDB _rev conflicts.
    lock: Arc<Mutex<()>>,
    /// Timestamp of the last successful persist, used for throttling rapid updates.
    last_persist: Option<Instant>,
}

impl PersistenceCoordinator {
    fn new() -> Self {
        Self {
            game_lock: Mutex::new(()),
            game_last_persist: RwLock::new(None),
            team_metadata: DashMap::new(),
        }
    }

    /// Clear all team persistence metadata.
    /// Should be called when switching to a new game to ensure clean state.
    fn clear_team_metadata(&self) {
        self.team_metadata.clear();
    }
}

/// Central application state storing persistent connections and database handles.
pub struct AppState {
    config: Arc<AppConfig>,
    game_store: RwLock<Option<Arc<dyn GameStore>>>,
    sse: SseState,
    buzzers: DashMap<String, BuzzerConnection>,
    game: RwLock<GameStateMachine>,
    current_game: RwLock<Option<GameSession>>,
    degraded_flag: RwLock<bool>,
    degraded_tx: watch::Sender<bool>,
    transition_gate: Mutex<()>,
    transition_timeout: Option<Duration>,
    persistence: PersistenceCoordinator,
}

impl AppState {
    /// Construct a new [`AppState`] wrapped in an [`Arc`] so it can be cloned cheaply.
    ///
    /// The application starts in degraded mode until a storage backend is installed.
    pub fn new() -> SharedState {
        let (degraded_tx, _rx) = watch::channel(true);
        Arc::new(Self {
            config: Arc::new(AppConfig::load()),
            game_store: RwLock::new(None),
            sse: SseState::new(16, 16),
            buzzers: DashMap::new(),
            game: RwLock::new(GameStateMachine::new()),
            current_game: RwLock::new(None),
            degraded_flag: RwLock::new(true),
            degraded_tx,
            transition_gate: Mutex::new(()),
            transition_timeout: Some(DEFAULT_TRANSITION_TIMEOUT),
            persistence: PersistenceCoordinator::new(),
        })
    }

    /// Retrieve the configured game store or report degraded mode.
    pub async fn require_game_store(&self) -> Result<Arc<dyn GameStore>, ServiceError> {
        let guard = self.game_store.read().await;
        guard.as_ref().cloned().ok_or(ServiceError::Degraded)
    }

    /// Helper to execute a persistence operation with locking and throttling.
    /// Takes a closure that performs the actual storage operation.
    async fn persist_with_throttle<F, Fut>(&self, persist_fn: F) -> Result<(), ServiceError>
    where
        F: FnOnce(Arc<dyn GameStore>, GameSession) -> Fut,
        Fut: std::future::Future<Output = Result<(), crate::dao::storage::StorageError>>,
    {
        // Serialize persistent saves so we don't issue concurrent PUTs to CouchDB which would
        // result in revision conflicts. We also throttle frequent calls: if a successful save
        // occurred recently, skip another save.
        let _lock = self.persistence.game_lock.lock().await;

        // Throttle window (tunable).
        const PERSIST_COOLDOWN: Duration = Duration::from_millis(200);

        if let Some(last) = *self.persistence.game_last_persist.read().await {
            if last.elapsed() < PERSIST_COOLDOWN {
                // A recent persist was performed; skip this write to avoid contention.
                return Ok(());
            }
        }

        let store = self.require_game_store().await?;
        let snapshot = {
            let guard = self.current_game.read().await;
            guard
                .as_ref()
                .cloned()
                .ok_or_else(|| ServiceError::InvalidState("no active game".into()))?
        };

        persist_fn(store, snapshot).await?;

        *self.persistence.game_last_persist.write().await = Some(Instant::now());
        Ok(())
    }

    /// Persist the current in-memory game back into the configured store.
    pub async fn persist_current_game(&self) -> Result<(), ServiceError> {
        self.persist_with_throttle(|store, snapshot| async move {
            store.save_game(snapshot.into()).await
        })
        .await
    }

    /// Persist only game document (without team documents) for efficient partial updates.
    /// Use this when only game-level fields have changed (e.g., current_song_index,
    /// current_song_found, playlist_song_order, found fields).
    /// The `teams` field in the snapshot is ignored by the storage layer.
    pub async fn persist_current_game_without_teams(&self) -> Result<(), ServiceError> {
        self.persist_with_throttle(|store, snapshot| async move {
            store.save_game_without_teams(snapshot.into()).await
        })
        .await
    }

    /// Persist a single team document to storage.
    /// Use this when only team-specific data has changed (e.g., score, name, buzzer_id).
    /// This method throttles per-team to avoid rapid-fire updates for the same team
    /// (e.g., from rapid score adjustments via REST API).
    /// Uses per-team locking so different teams can persist concurrently.
    pub async fn persist_team(
        &self,
        game_id: Uuid,
        team_id: Uuid,
        team: game::Team,
    ) -> Result<(), ServiceError> {
        const TEAM_PERSIST_COOLDOWN: Duration = Duration::from_millis(200);

        // Get or create metadata for this specific team
        let metadata = self
            .persistence
            .team_metadata
            .entry(team_id)
            .or_insert_with(|| TeamPersistMetadata {
                lock: Arc::new(Mutex::new(())),
                last_persist: None,
            });

        // Check throttle without holding the lock (fast path)
        if let Some(last) = metadata.last_persist {
            if last.elapsed() < TEAM_PERSIST_COOLDOWN {
                // Recent persist for this team; skip to avoid contention
                return Ok(());
            }
        }

        // Clone the lock to release the DashMap entry before awaiting
        let team_lock = metadata.lock.clone();
        drop(metadata);

        // Lock only this specific team, allowing other teams to persist concurrently
        let _lock = team_lock.lock().await;

        // Double-check throttle after acquiring lock (race condition mitigation)
        if let Some(metadata) = self.persistence.team_metadata.get(&team_id) {
            if let Some(last) = metadata.last_persist {
                if last.elapsed() < TEAM_PERSIST_COOLDOWN {
                    return Ok(());
                }
            }
        }

        let store = self.require_game_store().await?;
        let team_entity: TeamEntity = (team_id, team).into();
        store.save_team(game_id, team_entity).await?;

        // Update the per-team throttle timestamp
        if let Some(mut metadata) = self.persistence.team_metadata.get_mut(&team_id) {
            metadata.last_persist = Some(Instant::now());
        }

        Ok(())
    }

    /// Delete a single team document from storage.
    /// Uses per-team locking so different teams can be deleted concurrently.
    pub async fn delete_team(&self, game_id: Uuid, team_id: Uuid) -> Result<(), ServiceError> {
        // Get or create metadata for this specific team
        let team_lock = self
            .persistence
            .team_metadata
            .entry(team_id)
            .or_insert_with(|| TeamPersistMetadata {
                lock: Arc::new(Mutex::new(())),
                last_persist: None,
            })
            .lock
            .clone();

        // Lock only this specific team
        let _lock = team_lock.lock().await;

        let store = self.require_game_store().await?;
        store.delete_team(game_id, team_id).await?;

        // Clean up the metadata entry for this deleted team
        self.persistence.team_metadata.remove(&team_id);

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

    /// Access the immutable application configuration.
    pub fn config(&self) -> Arc<AppConfig> {
        Arc::clone(&self.config)
    }

    /// Retrieve a configured buzzer pattern to broadcast to devices.
    ///
    /// The provided `preset` carries the team color when the pattern needs to adopt a
    /// team-specific hue (e.g. standby/playing/answering effects).
    pub fn buzzer_pattern(&self, preset: BuzzerPatternPreset) -> BuzzerPattern {
        self.config.buzzer_pattern(preset)
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

    /// Mutate the in-memory game session, returning the closure result.
    ///
    /// The provided closure must remain synchronous; it is executed while the
    /// write lock on the current game is held. Returning any data needed for
    /// subsequent async work allows the lock to be released before awaiting.
    pub async fn with_current_game_mut<F, R>(&self, f: F) -> Result<R, ServiceError>
    where
        F: FnOnce(&mut GameSession) -> Result<R, ServiceError>,
    {
        self.with_current_game_slot_mut(|slot| {
            let game = slot
                .as_mut()
                .ok_or_else(|| ServiceError::InvalidState("no active game".into()))?;
            f(game)
        })
        .await
    }

    /// Read the optional current game slot.
    pub async fn read_current_game<F, R>(&self, f: F) -> R
    where
        F: FnOnce(Option<&GameSession>) -> R,
    {
        let guard = self.current_game.read().await;
        f(guard.as_ref())
    }

    /// Borrow the active game immutably, returning an error if none is present.
    pub async fn with_current_game<F, R>(&self, f: F) -> Result<R, ServiceError>
    where
        F: FnOnce(&GameSession) -> Result<R, ServiceError>,
    {
        self.read_current_game(|maybe| match maybe {
            Some(game) => f(game),
            None => Err(ServiceError::InvalidState("no active game".into())),
        })
        .await
    }

    /// Mutate the optional current game slot directly.
    pub async fn with_current_game_slot_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Option<GameSession>) -> R,
    {
        let mut guard = self.current_game.write().await;
        f(&mut guard)
    }

    /// Clear all team persistence metadata.
    /// Should be called when switching to a new game to ensure clean state.
    pub fn clear_team_metadata(&self) {
        self.persistence.clear_team_metadata();
    }

    /// Build a snapshot describing the current gameplay phase and related state.
    pub async fn game_phase_snapshot(&self, phase: &GamePhase) -> GamePhaseSnapshot {
        let phase_visible = VisibleGamePhase::from(phase);
        let game_id = self.read_current_game(|game| game.map(|g| g.id)).await;
        let degraded = self.is_degraded().await;

        let pairing_team_id = match phase {
            GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Pairing(session))) => {
                Some(session.pairing_team_id)
            }
            _ => None,
        };

        let paused_buzzer = match phase {
            GamePhase::GameRunning(GameRunningPhase::Paused(PauseKind::Buzz { id })) => {
                Some(id.clone())
            }
            _ => None,
        };

        let mut song = None;
        let mut scoreboard = None;
        let mut found_point_fields = None;
        let mut found_bonus_fields = None;

        let need_song = matches!(
            phase,
            GamePhase::GameRunning(GameRunningPhase::Playing)
                | GamePhase::GameRunning(GameRunningPhase::Reveal)
        );
        let need_found_fields = need_song;
        let need_scoreboard = matches!(phase, GamePhase::ShowScores);

        if need_song || need_found_fields || need_scoreboard {
            let (session_song, session_scoreboard, session_point_fields, session_bonus_fields) =
                self.read_current_game(|maybe| {
                    if let Some(game) = maybe {
                        (
                            if need_song {
                                current_song_snapshot(game)
                            } else {
                                None
                            },
                            if need_scoreboard {
                                Some(teams_to_summaries(&game.teams))
                            } else {
                                None
                            },
                            if need_found_fields {
                                Some(game.found_point_fields.clone())
                            } else {
                                None
                            },
                            if need_found_fields {
                                Some(game.found_bonus_fields.clone())
                            } else {
                                None
                            },
                        )
                    } else {
                        (None, None, None, None)
                    }
                })
                .await;

            song = session_song;
            scoreboard = session_scoreboard;
            found_point_fields = session_point_fields;
            found_bonus_fields = session_bonus_fields;
        }

        GamePhaseSnapshot {
            phase: phase_visible,
            game_id,
            degraded,
            pairing_team_id,
            paused_buzzer,
            song,
            scoreboard,
            found_point_fields,
            found_bonus_fields,
        }
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
}

fn teams_to_summaries(teams: &IndexMap<Uuid, Team>) -> Vec<TeamSummary> {
    teams.clone().into_iter().map(TeamSummary::from).collect()
}

fn current_song_snapshot(game: &GameSession) -> Option<SongSnapshot> {
    let index = game.current_song_index?;
    let song_id = *game.playlist_song_order.get(index)?;
    let song = game.playlist.songs.get(&song_id)?;
    Some(SongSnapshot::from_game_song(song_id, song))
}
