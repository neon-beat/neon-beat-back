//! Application state management and persistence coordination.
//!
//! This module provides the central `AppState` structure that coordinates all aspects
//! of the game application, including game state, persistence, real-time connections,
//! and state machine transitions.
//!
//! ## Persistence Architecture
//!
//! The persistence layer implements a sophisticated debouncing mechanism to balance
//! data consistency with database efficiency:
//!
//! ### Why Debouncing?
//!
//! **Problem**: Without debouncing, rapid-fire updates (e.g., score changes via REST API)
//! would either:
//! 1. Overwhelm the database with unnecessary writes
//! 2. OR silently drop updates during throttling, causing data loss
//!
//! **Solution**: Debouncing tracks pending updates during cooldown periods and ensures
//! they are eventually persisted after the cooldown expires.
//!
//! ### How It Works
//!
//! ```text
//! Timeline with 200ms cooldown:
//!
//! T=0ms:   persist_team() → Saves to DB immediately ✓
//! T=50ms:  persist_team() → Stores as pending, schedules flush at T=200ms
//! T=100ms: persist_team() → Replaces pending (latest state wins)
//! T=150ms: persist_team() → Replaces pending (latest state wins)
//! T=200ms: Flush task → Saves final state (T=150 data) to DB ✓
//!
//! Result: Only 2 DB writes for 4 update requests, but NO data loss!
//! ```
//!
//! ### Guarantees
//!
//! - **Eventual Consistency**: All updates are eventually persisted
//! - **Latest State Wins**: Only the most recent update is saved
//! - **No Redundant Tasks**: Only one flush task per cooldown window
//! - **Per-Team Concurrency**: Different teams can persist independently
//! - **Graceful Shutdown**: Pending updates are flushed before shutdown
//!
//! ### Tradeoffs
//!
//! - **Slight Delay**: Updates may take up to `COOLDOWN` ms to persist (default: 200ms)
//! - **Memory Overhead**: Pending updates are held in memory until flushed
//! - **Complexity**: More complex than simple throttling
//!
//! ### Configuration
//!
//! Cooldown duration is currently hardcoded at 200ms but can be made configurable
//! via `AppConfig` if different environments require different values.
//!
//! ## Graceful Shutdown
//!
//! The `shutdown()` method ensures all pending updates are flushed before the
//! application terminates, preventing data loss on restart.

/// Game session data structures and conversions.
pub mod game;
/// Server-Sent Events hub and state management.
mod sse;
/// State machine for game phase transitions.
pub mod state_machine;
/// State machine transition implementations.
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
use tracing::{info, warn};
use uuid::Uuid;

pub use self::sse::SseHub;
pub use self::state_machine::{AbortError, ApplyError, Plan, PlanError, PlanId, Snapshot};
use self::{
    sse::SseState,
    state_machine::{GameEvent, GameStateMachine},
};

/// Shared reference to application state, safe to clone across tasks.
pub type SharedState = Arc<AppState>;
/// Default timeout for state machine transitions.
pub const DEFAULT_TRANSITION_TIMEOUT: Duration = Duration::from_secs(5);

/// Handle used to push messages to a connected buzzer.
#[derive(Clone)]
pub struct BuzzerConnection {
    /// Unique identifier for the buzzer device.
    pub id: String,
    /// Channel sender for pushing messages to the buzzer WebSocket.
    pub tx: mpsc::UnboundedSender<Message>,
}

/// Coordinates persistence operations with locking, throttling, and debouncing.
///
/// ## Purpose
///
/// This component encapsulates all persistence coordination state to prevent:
/// - **Concurrent writes** causing CouchDB revision conflicts
/// - **Rapid-fire updates** overwhelming the database
/// - **Silent data loss** during throttling periods
///
/// ## Architecture
///
/// Uses separate coordination for game-level and team-level persistence to allow
/// different teams to persist concurrently while maintaining serial writes per team.
///
/// ## Debouncing Mechanism
///
/// When an update arrives during the cooldown window:
/// 1. The update is stored in the `pending` field
/// 2. If no flush task is running, one is spawned to wait for the cooldown
/// 3. Subsequent updates during cooldown replace the pending value (latest wins)
/// 4. After cooldown expires, the flush task persists the final pending state
/// 5. The `flush_scheduled` flag prevents spawning redundant flush tasks
///
/// ## Graceful Shutdown
///
/// Call `flush_all_pending()` before shutdown to ensure all pending updates are saved.
struct PersistenceCoordinator {
    /// Mutex used to serialize full game persistent saves to avoid concurrent PUTs.
    game_lock: Mutex<()>,
    /// Timestamp of last successful game persist, used for throttling.
    game_last_persist: RwLock<Option<Instant>>,
    /// Pending full game save that should be flushed after cooldown expires.
    pending_game: RwLock<Option<GameSession>>,
    /// Flag indicating whether a flush task is already scheduled for the game.
    game_flush_scheduled: RwLock<bool>,
    /// Per-team persistence metadata (lock + throttle timestamp + pending update).
    /// Keyed by team_id only since only one game is active at a time.
    team_metadata: DashMap<Uuid, TeamPersistMetadata>,
}

/// Metadata for coordinating team persistence operations.
/// Encapsulates the lock (for serialization), throttle timestamp (for rate limiting),
/// and pending update (for debouncing).
struct TeamPersistMetadata {
    /// Lock ensuring serial saves of this team document to avoid CouchDB _rev conflicts.
    lock: Arc<Mutex<()>>,
    /// Timestamp of the last successful persist, used for throttling rapid updates.
    last_persist: Option<Instant>,
    /// Pending team update that should be flushed after cooldown expires.
    pending: Option<Team>,
    /// Flag indicating whether a flush task is already scheduled for this team.
    flush_scheduled: bool,
}

impl PersistenceCoordinator {
    fn new() -> Self {
        Self {
            game_lock: Mutex::new(()),
            game_last_persist: RwLock::new(None),
            pending_game: RwLock::new(None),
            game_flush_scheduled: RwLock::new(false),
            team_metadata: DashMap::new(),
        }
    }

    /// Clear all persistence state in preparation for a new game session.
    ///
    /// This ensures that throttling, pending updates, and flush scheduling from the
    /// previous game don't interfere with the new game. This prevents issues like:
    /// - New game's first persist being throttled by old game's timing
    /// - Stale pending updates from previous game being flushed
    /// - Flush tasks from old game still running
    async fn clear_all(&self) {
        // Clear game-level state
        *self.game_last_persist.write().await = None;
        *self.pending_game.write().await = None;
        *self.game_flush_scheduled.write().await = false;

        // Clear team-level state
        self.team_metadata.clear();
    }
}

/// Central application state storing persistent connections and database handles.
pub struct AppState {
    config: Arc<AppConfig>,
    game_store: RwLock<Option<Arc<dyn GameStore>>>,
    sse: SseState,
    buzzers: DashMap<String, BuzzerConnection>,
    /// Last known pattern for each buzzer. This is updated on every successful pattern send
    /// and used to restore buzzer state when they reconnect.
    /// Tracks the desired state for each buzzer regardless of connection status.
    buzzer_last_patterns: DashMap<String, BuzzerPatternPreset>,
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
            buzzer_last_patterns: DashMap::new(),
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

    /// Helper to execute a persistence operation with locking, throttling, and debouncing.
    ///
    /// ## Behavior
    ///
    /// - **Immediate persist**: If no recent persist occurred, saves immediately
    /// - **Debounced persist**: If within cooldown window, stores as pending and schedules flush
    /// - **Cooldown**: 200ms (prevents more than 5 writes/second per entity)
    ///
    /// ## Debouncing Details
    ///
    /// When an update arrives during cooldown:
    /// 1. Current game snapshot is stored in `pending_game`
    /// 2. If no flush task is scheduled, spawn one to wait for remaining cooldown
    /// 3. Subsequent updates replace the pending snapshot (latest wins)
    /// 4. After cooldown, flush task persists the final state
    ///
    /// This ensures:
    /// - No data loss (all updates eventually persisted)
    /// - Efficient DB usage (throttled writes)
    /// - Latest state wins (most recent data is saved)
    ///
    /// ## Parameters
    ///
    /// - `persist_fn`: Closure that performs the actual storage operation
    async fn persist_with_throttle<F, Fut>(
        self: &Arc<Self>,
        persist_fn: F,
    ) -> Result<(), ServiceError>
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
                // Recent persist occurred; store as pending
                let remaining = PERSIST_COOLDOWN - last.elapsed();

                let snapshot = {
                    let guard = self.current_game.read().await;
                    guard
                        .as_ref()
                        .cloned()
                        .ok_or_else(|| ServiceError::InvalidState("no active game".into()))?
                };

                // Only spawn flush task if one isn't already scheduled
                let should_spawn = {
                    let mut scheduled = self.persistence.game_flush_scheduled.write().await;
                    let currently_scheduled = *scheduled;
                    if !currently_scheduled {
                        *scheduled = true;
                    }
                    !currently_scheduled
                };

                {
                    let mut pending = self.persistence.pending_game.write().await;
                    *pending = Some(snapshot);
                }

                drop(_lock);

                if should_spawn {
                    // Spawn task to flush pending update after cooldown
                    let state = Arc::clone(self);
                    tokio::spawn(async move {
                        tokio::time::sleep(remaining).await;
                        if let Err(e) = state.flush_pending_game().await {
                            warn!(
                                error = ?e,
                                "failed to flush pending game update"
                            );
                        }
                    });
                }

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
    pub async fn persist_current_game(self: &Arc<Self>) -> Result<(), ServiceError> {
        self.persist_with_throttle(|store, snapshot| async move {
            store.save_game(snapshot.into()).await
        })
        .await
    }

    /// Persist only game document (without team documents) for efficient partial updates.
    /// Use this when only game-level fields have changed (e.g., current_song_index,
    /// current_song_found, playlist_song_order, found fields).
    /// The `teams` field in the snapshot is ignored by the storage layer.
    pub async fn persist_current_game_without_teams(self: &Arc<Self>) -> Result<(), ServiceError> {
        self.persist_with_throttle(|store, snapshot| async move {
            store.save_game_without_teams(snapshot.into()).await
        })
        .await
    }

    /// Persist a single team document to storage with throttling and debouncing.
    ///
    /// ## Use Case
    ///
    /// Call this when only team-specific data has changed:
    /// - Score updates
    /// - Name changes
    /// - Buzzer ID assignments
    ///
    /// ## Concurrency
    ///
    /// - **Per-team locking**: Different teams can persist concurrently
    /// - **Serial writes**: Multiple updates to the same team are serialized
    ///
    /// ## Debouncing
    ///
    /// Rapid-fire updates (e.g., score spam via REST API) are debounced:
    /// - First update: persists immediately
    /// - Updates during cooldown (200ms): stored as pending
    /// - After cooldown: flush task persists the final state
    ///
    /// Example timeline for team A:
    /// ```text
    /// T=0ms:   score=10 → persists immediately
    /// T=50ms:  score=20 → pending, schedules flush at T=200ms
    /// T=100ms: score=30 → replaces pending
    /// T=150ms: score=40 → replaces pending
    /// T=200ms: flush → persists score=40
    /// ```
    ///
    /// ## Parameters
    ///
    /// - `game_id`: ID of the game containing this team
    /// - `team_id`: ID of the team to persist
    /// - `team`: The team data to save
    pub async fn persist_team(
        self: &Arc<Self>,
        game_id: Uuid,
        team_id: Uuid,
        team: game::Team,
    ) -> Result<(), ServiceError> {
        const TEAM_PERSIST_COOLDOWN: Duration = Duration::from_millis(200);

        // Get or create metadata for this specific team
        let mut metadata = self
            .persistence
            .team_metadata
            .entry(team_id)
            .or_insert_with(|| TeamPersistMetadata {
                lock: Arc::new(Mutex::new(())),
                last_persist: None,
                pending: None,
                flush_scheduled: false,
            });

        // Check throttle without holding the lock (fast path)
        if let Some(last) = metadata.last_persist {
            if last.elapsed() < TEAM_PERSIST_COOLDOWN {
                // Recent persist for this team; store as pending
                let remaining = TEAM_PERSIST_COOLDOWN - last.elapsed();
                metadata.pending = Some(team);

                // Only spawn flush task if one isn't already scheduled
                let should_spawn = !metadata.flush_scheduled;
                if should_spawn {
                    metadata.flush_scheduled = true;
                }
                drop(metadata);

                if should_spawn {
                    // Spawn task to flush pending update after cooldown
                    let state = Arc::clone(self);
                    let task = async move {
                        tokio::time::sleep(remaining).await;
                        if let Err(e) = state.flush_pending_team(game_id, team_id).await {
                            warn!(
                                game_id = %game_id,
                                team_id = %team_id,
                                error = ?e,
                                "failed to flush pending team update"
                            );
                        }
                    };
                    tokio::spawn(task);
                }

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
                    // Another task persisted while we were waiting for the lock
                    // Store as pending for the next flush cycle
                    drop(metadata);
                    if let Some(mut metadata) = self.persistence.team_metadata.get_mut(&team_id) {
                        let remaining = TEAM_PERSIST_COOLDOWN - last.elapsed();
                        metadata.pending = Some(team);

                        // Only spawn flush task if one isn't already scheduled
                        let should_spawn = !metadata.flush_scheduled;
                        if should_spawn {
                            metadata.flush_scheduled = true;
                            drop(metadata);

                            // Spawn task to flush this pending update
                            let state = Arc::clone(self);
                            tokio::spawn(async move {
                                tokio::time::sleep(remaining).await;
                                if let Err(e) = state.flush_pending_team(game_id, team_id).await {
                                    warn!(
                                        game_id = %game_id,
                                        team_id = %team_id,
                                        error = ?e,
                                        "failed to flush pending team update"
                                    );
                                }
                            });
                        }
                    }
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
                pending: None,
                flush_scheduled: false,
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

    /// Registry of last known patterns for all buzzers.
    /// This is updated on every successful pattern send and used to restore buzzer state on reconnection.
    pub fn buzzer_last_patterns(&self) -> &DashMap<String, BuzzerPatternPreset> {
        &self.buzzer_last_patterns
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

    /// Clear all game-scoped state in preparation for a new game session.
    ///
    /// This clears:
    /// - Persistence coordination state (throttling, pending updates, flush scheduling)
    /// - Buzzer pattern cache
    ///
    /// Should be called when creating or loading a new game to ensure that state
    /// from the previous game doesn't interfere with the new game.
    pub async fn clear_game_state(&self) {
        // Clear all persistence state
        self.persistence.clear_all().await;

        // Clear buzzer pattern cache
        self.buzzer_last_patterns.clear();
    }

    /// Flush any pending team update for the given team_id.
    /// Called by debounce tasks after cooldown expires to ensure eventual consistency.
    async fn flush_pending_team(
        self: &Arc<Self>,
        game_id: Uuid,
        team_id: Uuid,
    ) -> Result<(), ServiceError> {
        // Reset flag immediately to allow new flushes to be scheduled
        // This happens first to ensure the flag is reset even if persistence fails
        {
            if let Some(mut metadata) = self.persistence.team_metadata.get_mut(&team_id) {
                metadata.flush_scheduled = false;
            }
        }

        // Extract pending update if present
        let (pending_team, team_lock) = {
            let mut metadata = match self.persistence.team_metadata.get_mut(&team_id) {
                Some(m) => m,
                None => return Ok(()), // Metadata was cleared (game transition)
            };
            (metadata.pending.take(), metadata.lock.clone())
        };

        // If there's a pending update, persist it directly (don't call persist_team to avoid recursion)
        if let Some(team) = pending_team {
            // Lock to ensure serial writes
            let _lock = team_lock.lock().await;

            let store = self.require_game_store().await?;
            let team_entity: TeamEntity = (team_id, team).into();
            store.save_team(game_id, team_entity).await?;

            // Update timestamp
            if let Some(mut metadata) = self.persistence.team_metadata.get_mut(&team_id) {
                metadata.last_persist = Some(Instant::now());
            }
        }

        Ok(())
    }

    /// Flush any pending full game save.
    /// Called by debounce tasks after cooldown expires to ensure eventual consistency.
    async fn flush_pending_game(self: &Arc<Self>) -> Result<(), ServiceError> {
        // Extract pending game snapshot if present
        let pending_game = {
            let mut guard = self.persistence.pending_game.write().await;
            guard.take()
        };

        // Reset the flush scheduled flag so new updates can schedule a flush
        {
            let mut scheduled = self.persistence.game_flush_scheduled.write().await;
            *scheduled = false;
        }

        // If there's a pending update, persist it
        if let Some(game) = pending_game {
            // Persist the snapshot directly, bypassing throttle check
            // (we already waited the cooldown in the debounce task)
            let _lock = self.persistence.game_lock.lock().await;

            let store = self.require_game_store().await?;
            store.save_game(game.into()).await?;

            *self.persistence.game_last_persist.write().await = Some(Instant::now());
        }

        Ok(())
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

    /// Gracefully shutdown persistence operations by flushing all pending updates.
    ///
    /// This method ensures that any pending game or team updates that are waiting
    /// for the cooldown period are immediately persisted to the database before
    /// the application terminates.
    ///
    /// ## Behavior
    ///
    /// - Flushes pending full game save if present
    /// - Flushes all pending team updates across all teams
    /// - Bypasses cooldown checks (persistence happens immediately)
    /// - Logs any errors but continues flushing remaining updates
    ///
    /// ## When to Call
    ///
    /// Call this method during application shutdown, before dropping the `AppState`:
    ///
    /// ```ignore
    /// // In main.rs shutdown handler:
    /// state.shutdown().await;
    /// ```
    ///
    /// ## Error Handling
    ///
    /// Errors are logged but do not stop the flush process. This ensures that
    /// even if one team's data fails to persist, other teams' data is still saved.
    pub async fn shutdown(self: &Arc<Self>) -> Result<(), ServiceError> {
        info!("Starting graceful shutdown of persistence layer");

        let mut error_count = 0;
        let mut success_count = 0;

        // Flush pending game save
        if let Some(game) = self.persistence.pending_game.read().await.clone() {
            info!("Flushing pending game save during shutdown");
            match self.flush_game_immediate(game).await {
                Ok(_) => {
                    success_count += 1;
                    info!("Successfully flushed pending game save");
                }
                Err(e) => {
                    error_count += 1;
                    warn!(error = ?e, "Failed to flush pending game save during shutdown");
                }
            }
        }

        // Flush all pending team updates
        let team_ids: Vec<Uuid> = self
            .persistence
            .team_metadata
            .iter()
            .filter_map(|entry| {
                if entry.pending.is_some() {
                    Some(*entry.key())
                } else {
                    None
                }
            })
            .collect();

        if !team_ids.is_empty() {
            info!(
                count = team_ids.len(),
                "Flushing pending team updates during shutdown"
            );
        }

        for team_id in team_ids {
            // Get the current game_id - we need it to persist teams
            let game_id = match self.read_current_game(|game| game.map(|g| g.id)).await {
                Some(id) => id,
                None => {
                    warn!(team_id = %team_id, "Cannot flush team: no active game");
                    error_count += 1;
                    continue;
                }
            };

            // Extract the pending team
            let (pending_team, team_lock) = {
                match self.persistence.team_metadata.get_mut(&team_id) {
                    Some(mut metadata) => {
                        let pending = metadata.pending.take();
                        (pending, metadata.lock.clone())
                    }
                    None => continue,
                }
            };

            if let Some(team) = pending_team {
                // Lock and persist immediately
                let _lock = team_lock.lock().await;

                match self.require_game_store().await {
                    Ok(store) => {
                        let team_entity: TeamEntity = (team_id, team).into();
                        match store.save_team(game_id, team_entity).await {
                            Ok(_) => {
                                success_count += 1;
                                info!(team_id = %team_id, "Successfully flushed pending team update");
                            }
                            Err(e) => {
                                error_count += 1;
                                warn!(team_id = %team_id, error = ?e, "Failed to flush pending team update during shutdown");
                            }
                        }
                    }
                    Err(e) => {
                        error_count += 1;
                        warn!(team_id = %team_id, error = ?e, "Failed to get game store during shutdown");
                    }
                }
            }
        }

        if error_count > 0 {
            warn!(
                success = success_count,
                errors = error_count,
                "Graceful shutdown completed with errors"
            );
        } else if success_count > 0 {
            info!(
                success = success_count,
                "Graceful shutdown completed successfully"
            );
        } else {
            info!("Graceful shutdown completed (no pending updates)");
        }

        Ok(())
    }

    /// Immediately flush a game snapshot, bypassing cooldown checks.
    /// Used during graceful shutdown.
    async fn flush_game_immediate(&self, game: GameSession) -> Result<(), ServiceError> {
        let _lock = self.persistence.game_lock.lock().await;
        let store = self.require_game_store().await?;
        store.save_game(game.into()).await?;
        Ok(())
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

    /// Get a snapshot of the current state machine state.
    pub async fn snapshot(&self) -> Snapshot {
        let sm = self.game.read().await;
        sm.snapshot()
    }

    /// Run a state machine transition with custom work, applying the transition on success or aborting on failure.
    /// The work closure is executed after planning but before applying the transition.
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
