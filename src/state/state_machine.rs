use std::time::Instant;

use indexmap::IndexMap;
use thiserror::Error;
use uuid::Uuid;

use crate::state::game::Team;

/// High-level phases the game can be in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GamePhase {
    /// No game is currently running; playlists and teams can be managed.
    Idle,
    /// A game is active and can be in one of the gameplay sub-phases.
    GameRunning(GameRunningPhase),
    /// Final scoreboard is displayed before returning to idle.
    ShowScores,
}

/// Fine-grained phase while the game is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameRunningPhase {
    /// Pre-game configuration: teams, playlist, and assets are set up.
    Prep(PrepStatus),
    /// Actively playing the current song, buzzers enabled.
    Playing,
    /// Game is paused either manually or because a team buzzed in.
    Paused(PauseKind),
    /// The current song (or answer) is being revealed.
    Reveal,
}

/// Prep sub-mode data (ready or pairing with session data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepStatus {
    /// Ready to start the game.
    Ready,
    /// Currently pairing buzzers with teams.
    Pairing(PairingSession),
}

/// Information about an active buzzer pairing session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingSession {
    /// ID of the team currently pairing their buzzer.
    pub pairing_team_id: Uuid,
    /// Snapshot of teams at the start of pairing.
    pub snapshot: IndexMap<Uuid, Team>,
}

/// Represents why the game entered a paused state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PauseKind {
    /// The game master manually paused gameplay.
    Manual,
    /// Gameplay paused because a team buzzed in (id identifies the buzzer).
    Buzz {
        /// Identifier of the buzzer that buzzed.
        id: String,
    },
}

/// Indicates why gameplay transitioned to the final scoreboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishReason {
    /// Playlist reached the end naturally.
    PlaylistCompleted,
    /// Game master decided to stop the game early.
    ManualStop,
}

/// Events that can be applied to the state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameEvent {
    /// GM starts the game from the idle state.
    StartGame,
    /// Begin the pairing workflow while in prep.
    PairingStarted(PairingSession),
    /// Exit the pairing workflow and return to ready prep.
    PairingFinished,
    /// Configuration is done; enter active gameplay.
    GameConfigured,
    /// Pause gameplay, either manually or because of a buzz.
    Pause(PauseKind),
    /// Resume playing after a pause.
    ContinuePlaying,
    /// Reveal the answer for the current song.
    Reveal,
    /// Move to the next song after a reveal.
    NextSong,
    /// Transition to the final scoreboard view.
    Finish(FinishReason),
    /// Completely end the game and return to idle.
    EndGame,
}

/// Error returned when attempting to apply an invalid transition.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid transition: {event:?} cannot be applied while in {from:?}")]
pub struct InvalidTransition {
    /// The phase the state machine was in when the invalid event was received.
    pub from: GamePhase,
    /// The event that cannot be applied from this phase.
    pub event: GameEvent,
}

/// Errors that can occur when planning a state machine transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {
    /// A transition is already pending and must be applied or aborted.
    AlreadyPending,
    /// The requested transition is not valid from the current phase.
    InvalidTransition(InvalidTransition),
}

/// Errors that can occur when applying a planned state machine transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// No transition is currently pending.
    NoPending,
    /// Plan ID does not match the pending plan.
    IdMismatch {
        /// Expected plan ID.
        expected: PlanId,
        /// Provided plan ID.
        got: PlanId,
    },
    /// State machine phase changed since the plan was created.
    PhaseMismatch {
        /// Phase when plan was created.
        expected: GamePhase,
        /// Current phase.
        actual: GamePhase,
    },
    /// State machine version changed since the plan was created.
    VersionMismatch {
        /// Version when plan was created.
        expected: usize,
        /// Current version.
        actual: usize,
    },
}

/// Errors that can occur when aborting a planned state machine transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbortError {
    /// No transition is currently pending.
    NoPending,
    /// Plan ID does not match the pending plan.
    IdMismatch {
        /// Expected plan ID.
        expected: PlanId,
        /// Provided plan ID.
        got: PlanId,
    },
}

/// Unique identifier for a planned state transition.
pub type PlanId = Uuid;

/// A planned state machine transition that has been validated but not yet applied.
#[derive(Debug, Clone)]
pub struct Plan {
    /// Unique identifier for this plan.
    pub id: PlanId,
    /// Phase the state machine is currently in.
    pub from: GamePhase,
    /// Phase the state machine will transition to.
    pub to: GamePhase,
    /// Event that triggered this transition.
    pub event: GameEvent,
    /// Version number after applying this transition.
    pub version_next: usize,
    /// Timestamp when this plan was created.
    pub pending_since: Instant,
}

/// Snapshot of the current state machine state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Current phase of the state machine.
    pub phase: GamePhase,
    /// Version number of the state machine (increments on each transition).
    pub version: usize,
    /// Pending transition phase, if a transition is planned but not yet applied.
    pub pending: Option<GamePhase>,
}

/// State machine implementing the gameplay flow described in the README.
#[derive(Debug, Clone)]
pub struct GameStateMachine {
    phase: GamePhase,
    version: usize,
    pending: Option<Plan>,
}

impl Default for GameStateMachine {
    fn default() -> Self {
        Self {
            phase: GamePhase::Idle,
            version: 0,
            pending: None,
        }
    }
}

impl GameStateMachine {
    /// Create a new state machine initialised in the idle state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inspect the current phase.
    pub fn phase(&self) -> GamePhase {
        self.phase.clone()
    }

    /// Get an immutable reference to the current pairing session, if active.
    pub fn pairing_session(&self) -> Option<&PairingSession> {
        match &self.phase {
            GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Pairing(session))) => {
                Some(session)
            }
            _ => None,
        }
    }

    /// Get a mutable reference to the current pairing session, if active.
    pub fn pairing_session_mut(&mut self) -> Option<&mut PairingSession> {
        match &mut self.phase {
            GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Pairing(session))) => {
                Some(session)
            }
            _ => None,
        }
    }

    /// Create a snapshot of the current state machine state.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            phase: self.phase.clone(),
            version: self.version,
            pending: self.pending.as_ref().map(|plan| plan.to.clone()),
        }
    }

    /// Plan a transition by validating that the event can be applied from the current phase.
    /// Returns a Plan that can later be applied or aborted.
    pub fn plan(&mut self, event: GameEvent) -> Result<Plan, PlanError> {
        if self.pending.is_some() {
            return Err(PlanError::AlreadyPending);
        }

        let next = self
            .compute_transition(event.clone())
            .map_err(PlanError::InvalidTransition)?;

        let plan = Plan {
            id: Uuid::new_v4(),
            from: self.phase.clone(),
            to: next,
            event,
            version_next: self.version + 1,
            pending_since: Instant::now(),
        };

        self.pending = Some(plan.clone());

        Ok(plan)
    }

    /// Apply a planned transition, moving the state machine to the next phase.
    /// Returns the new phase after the transition.
    pub fn apply(&mut self, plan_id: PlanId) -> Result<GamePhase, ApplyError> {
        let plan = self.pending.take().ok_or(ApplyError::NoPending)?;

        if plan.id != plan_id {
            let expected_plan_id = plan.id;
            self.pending = Some(plan);
            return Err(ApplyError::IdMismatch {
                expected: expected_plan_id,
                got: plan_id,
            });
        }

        if self.phase != plan.from {
            return Err(ApplyError::PhaseMismatch {
                expected: plan.from,
                actual: self.phase.clone(),
            });
        }

        if self.version + 1 != plan.version_next {
            return Err(ApplyError::VersionMismatch {
                expected: plan.version_next,
                actual: self.version + 1,
            });
        }

        self.phase = plan.to;
        self.version = plan.version_next;
        self.pending = None;

        Ok(self.phase.clone())
    }

    /// Abort a planned transition without applying it, returning the state machine to its previous state.
    pub fn abort(&mut self, plan_id: PlanId) -> Result<(), AbortError> {
        let plan = self.pending.as_ref().ok_or(AbortError::NoPending)?;

        if plan.id != plan_id {
            return Err(AbortError::IdMismatch {
                expected: plan.id,
                got: plan_id,
            });
        }

        self.pending = None;
        Ok(())
    }

    /// Compute a transition from an event if the transition is valid.
    fn compute_transition(&self, event: GameEvent) -> Result<GamePhase, InvalidTransition> {
        let next = match (self.phase.clone(), event) {
            (GamePhase::Idle, GameEvent::StartGame) => {
                GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Ready))
            }
            (
                GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Ready)),
                GameEvent::PairingStarted(session),
            ) => GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Pairing(session))),
            (
                GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Pairing(_))),
                GameEvent::PairingFinished,
            ) => GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Ready)),
            (
                GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Ready)),
                GameEvent::GameConfigured,
            ) => GamePhase::GameRunning(GameRunningPhase::Playing),
            (GamePhase::GameRunning(GameRunningPhase::Playing), GameEvent::Pause(kind)) => {
                GamePhase::GameRunning(GameRunningPhase::Paused(kind))
            }
            (GamePhase::GameRunning(GameRunningPhase::Playing), GameEvent::Reveal) => {
                GamePhase::GameRunning(GameRunningPhase::Reveal)
            }
            (GamePhase::GameRunning(GameRunningPhase::Paused(..)), GameEvent::ContinuePlaying) => {
                GamePhase::GameRunning(GameRunningPhase::Playing)
            }
            (GamePhase::GameRunning(GameRunningPhase::Paused(..)), GameEvent::Reveal) => {
                GamePhase::GameRunning(GameRunningPhase::Reveal)
            }
            (GamePhase::GameRunning(GameRunningPhase::Reveal), GameEvent::NextSong) => {
                GamePhase::GameRunning(GameRunningPhase::Playing)
            }
            (GamePhase::GameRunning(_), GameEvent::Finish(..)) => GamePhase::ShowScores,
            (GamePhase::ShowScores, GameEvent::EndGame) => GamePhase::Idle,
            (from, event) => return Err(InvalidTransition { from, event }),
        };

        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(sm: &mut GameStateMachine, event: GameEvent) -> GamePhase {
        let plan = sm.plan(event).unwrap();
        sm.apply(plan.id).unwrap()
    }

    #[test]
    fn initial_state_is_idle() {
        let sm = GameStateMachine::new();
        assert_eq!(sm.phase(), GamePhase::Idle);
    }

    #[test]
    fn full_happy_path_through_game() {
        let mut sm = GameStateMachine::new();

        assert_eq!(
            apply(&mut sm, GameEvent::StartGame),
            GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Ready))
        );
        assert_eq!(
            apply(&mut sm, GameEvent::GameConfigured),
            GamePhase::GameRunning(GameRunningPhase::Playing)
        );
        assert_eq!(
            apply(&mut sm, GameEvent::Pause(PauseKind::Manual)),
            GamePhase::GameRunning(GameRunningPhase::Paused(PauseKind::Manual))
        );
        assert_eq!(
            apply(&mut sm, GameEvent::Reveal),
            GamePhase::GameRunning(GameRunningPhase::Reveal)
        );
        assert_eq!(
            apply(&mut sm, GameEvent::NextSong),
            GamePhase::GameRunning(GameRunningPhase::Playing)
        );

        assert_eq!(
            apply(&mut sm, GameEvent::Finish(FinishReason::PlaylistCompleted)),
            GamePhase::ShowScores
        );
        assert_eq!(apply(&mut sm, GameEvent::EndGame), GamePhase::Idle);
    }

    #[test]
    fn buzzing_causes_pause_and_effect() {
        let mut sm = GameStateMachine::new();
        apply(&mut sm, GameEvent::StartGame);
        apply(&mut sm, GameEvent::GameConfigured);

        let plan = sm.plan(GameEvent::Pause(PauseKind::Buzz {
            id: "deadbeef0001".into(),
        }));
        let plan = plan.unwrap();
        let next = sm.apply(plan.id).unwrap();

        match next {
            GamePhase::GameRunning(GameRunningPhase::Paused(PauseKind::Buzz { id })) => {
                assert_eq!(id, "deadbeef0001")
            }
            other => panic!("expected pause with buzz id, got {other:?}"),
        }
    }

    #[test]
    fn continue_playing_after_buzz_triggers_effect() {
        let mut sm = GameStateMachine::new();
        apply(&mut sm, GameEvent::StartGame);
        apply(&mut sm, GameEvent::GameConfigured);
        apply(
            &mut sm,
            GameEvent::Pause(PauseKind::Buzz {
                id: "deadbeef0002".into(),
            }),
        );

        let plan = sm.plan(GameEvent::ContinuePlaying).unwrap();
        let next = sm.apply(plan.id).unwrap();
        assert_eq!(next, GamePhase::GameRunning(GameRunningPhase::Playing));
    }

    #[test]
    fn reveal_after_buzz_triggers_effect() {
        let mut sm = GameStateMachine::new();
        apply(&mut sm, GameEvent::StartGame);
        apply(&mut sm, GameEvent::GameConfigured);
        apply(
            &mut sm,
            GameEvent::Pause(PauseKind::Buzz {
                id: "deadbeef0003".into(),
            }),
        );

        let plan = sm.plan(GameEvent::Reveal).unwrap();
        let next = sm.apply(plan.id).unwrap();
        assert_eq!(next, GamePhase::GameRunning(GameRunningPhase::Reveal));
    }

    #[test]
    fn pairing_transitions_enforced() {
        let mut sm = GameStateMachine::new();
        let pairing_session = PairingSession {
            pairing_team_id: Uuid::new_v4(),
            snapshot: IndexMap::new(),
        };

        assert_eq!(
            apply(&mut sm, GameEvent::StartGame),
            GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Ready))
        );

        assert_eq!(
            apply(&mut sm, GameEvent::PairingStarted(pairing_session.clone())),
            GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Pairing(
                pairing_session.clone()
            )))
        );

        let err = sm.plan(GameEvent::GameConfigured).unwrap_err();
        match err {
            PlanError::InvalidTransition(InvalidTransition { from, event }) => {
                assert_eq!(
                    from,
                    GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Pairing(
                        pairing_session.clone()
                    )))
                );
                assert_eq!(event, GameEvent::GameConfigured);
            }
            other => panic!("unexpected error: {other:?}"),
        }

        assert_eq!(
            apply(&mut sm, GameEvent::PairingFinished),
            GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Ready))
        );
    }

    #[test]
    fn invalid_transition_returns_error() {
        let mut sm = GameStateMachine::new();
        let err = sm.plan(GameEvent::Reveal).unwrap_err();
        match err {
            PlanError::InvalidTransition(invalid) => {
                assert_eq!(invalid.from, GamePhase::Idle);
                assert_eq!(invalid.event, GameEvent::Reveal);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn abort_clears_pending() {
        let mut sm = GameStateMachine::new();
        let plan = sm.plan(GameEvent::StartGame).unwrap();
        sm.abort(plan.id).unwrap();
        assert!(sm.pending.is_none());
    }
}
