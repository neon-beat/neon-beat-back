use thiserror::Error;

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
    Prep,
    /// Actively playing the current song, buzzers enabled.
    Playing,
    /// Game is paused either manually or because a team buzzed in.
    Paused(PauseKind),
    /// The current song (or answer) is being revealed.
    Reveal,
}

/// Represents why the game entered a paused state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PauseKind {
    /// The game master manually paused gameplay.
    Manual,
    /// Gameplay paused because a team buzzed in (id identifies the buzzer).
    Buzz { id: String },
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
    pub from: GamePhase,
    pub event: GameEvent,
}

/// State machine implementing the gameplay flow described in the README.
#[derive(Debug, Clone)]
pub struct GameStateMachine {
    phase: GamePhase,
    last_finish_reason: Option<FinishReason>,
}

impl Default for GameStateMachine {
    fn default() -> Self {
        Self {
            phase: GamePhase::Idle,
            last_finish_reason: None,
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

    /// Last recorded reason used when leaving gameplay for the scoreboard.
    pub fn last_finish_reason(&self) -> Option<FinishReason> {
        self.last_finish_reason
    }

    /// Apply an event and update the underlying phase if the transition is valid.
    pub fn apply(&mut self, event: GameEvent) -> Result<GamePhase, InvalidTransition> {
        let next = match (self.phase.clone(), event) {
            (GamePhase::Idle, GameEvent::StartGame) => {
                GamePhase::GameRunning(GameRunningPhase::Prep)
            }
            (GamePhase::GameRunning(GameRunningPhase::Prep), GameEvent::GameConfigured) => {
                GamePhase::GameRunning(GameRunningPhase::Playing)
            }
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
            (GamePhase::GameRunning(_), GameEvent::Finish(reason)) => {
                self.last_finish_reason = Some(reason);
                GamePhase::ShowScores
            }
            (GamePhase::ShowScores, GameEvent::EndGame) => {
                self.last_finish_reason = None;
                GamePhase::Idle
            }
            (from, event) => {
                return Err(InvalidTransition { from, event });
            }
        };

        self.phase = next.clone();
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_idle() {
        let sm = GameStateMachine::new();
        assert_eq!(sm.phase(), GamePhase::Idle);
        assert_eq!(sm.last_finish_reason(), None);
    }

    #[test]
    fn full_happy_path_through_game() {
        let mut sm = GameStateMachine::new();

        assert_eq!(
            sm.apply(GameEvent::StartGame).unwrap(),
            GamePhase::GameRunning(GameRunningPhase::Prep)
        );
        assert_eq!(
            sm.apply(GameEvent::GameConfigured).unwrap(),
            GamePhase::GameRunning(GameRunningPhase::Playing)
        );
        assert_eq!(
            sm.apply(GameEvent::Pause(PauseKind::Manual)).unwrap(),
            GamePhase::GameRunning(GameRunningPhase::Paused(PauseKind::Manual))
        );
        assert_eq!(
            sm.apply(GameEvent::Reveal).unwrap(),
            GamePhase::GameRunning(GameRunningPhase::Reveal)
        );
        assert_eq!(
            sm.apply(GameEvent::NextSong).unwrap(),
            GamePhase::GameRunning(GameRunningPhase::Playing)
        );
        assert_eq!(
            sm.apply(GameEvent::Finish(FinishReason::PlaylistCompleted))
                .unwrap(),
            GamePhase::ShowScores
        );
        assert_eq!(
            sm.last_finish_reason(),
            Some(FinishReason::PlaylistCompleted)
        );
        assert_eq!(sm.apply(GameEvent::EndGame).unwrap(), GamePhase::Idle);
        assert_eq!(sm.last_finish_reason(), None);
    }

    #[test]
    fn buzzing_causes_pause() {
        let mut sm = GameStateMachine::new();
        sm.apply(GameEvent::StartGame).unwrap();
        sm.apply(GameEvent::GameConfigured).unwrap();

        let buzzer_id = "deadbeef0001";

        match sm
            .apply(GameEvent::Pause(PauseKind::Buzz {
                id: buzzer_id.to_string(),
            }))
            .unwrap()
        {
            GamePhase::GameRunning(GameRunningPhase::Paused(PauseKind::Buzz { id })) => {
                assert_eq!(id, buzzer_id);
            }
            other => panic!("expected pause with buzz id, got {other:?}"),
        }
    }

    #[test]
    fn invalid_transition_returns_error() {
        let mut sm = GameStateMachine::new();
        let err = sm.apply(GameEvent::Reveal).unwrap_err();
        assert_eq!(err.from, GamePhase::Idle);
        assert_eq!(err.event, GameEvent::Reveal);
    }

    #[test]
    fn resume_requires_pause() {
        let mut sm = GameStateMachine::new();
        sm.apply(GameEvent::StartGame).unwrap();
        sm.apply(GameEvent::GameConfigured).unwrap();

        let err = sm.apply(GameEvent::ContinuePlaying).unwrap_err();
        assert_eq!(err.from, GamePhase::GameRunning(GameRunningPhase::Playing));
        assert_eq!(err.event, GameEvent::ContinuePlaying);
    }

    #[test]
    fn finish_allowed_from_pause_state() {
        let mut sm = GameStateMachine::new();
        sm.apply(GameEvent::StartGame).unwrap();
        sm.apply(GameEvent::GameConfigured).unwrap();
        sm.apply(GameEvent::Pause(PauseKind::Manual)).unwrap();

        assert_eq!(
            sm.apply(GameEvent::Finish(FinishReason::ManualStop))
                .unwrap(),
            GamePhase::ShowScores
        );
        assert_eq!(sm.last_finish_reason(), Some(FinishReason::ManualStop));
    }

    #[test]
    fn continue_playing_after_buzz_has_side_effect() {
        let mut sm = GameStateMachine::new();
        sm.apply(GameEvent::StartGame).unwrap();
        sm.apply(GameEvent::GameConfigured).unwrap();
        sm.apply(GameEvent::Pause(PauseKind::Buzz {
            id: "deadbeef0001".into(),
        }))
        .unwrap();

        assert_eq!(
            sm.apply(GameEvent::ContinuePlaying).unwrap(),
            GamePhase::GameRunning(GameRunningPhase::Playing)
        );
    }

    #[test]
    fn reveal_after_buzz_has_side_effect() {
        let mut sm = GameStateMachine::new();
        sm.apply(GameEvent::StartGame).unwrap();
        sm.apply(GameEvent::GameConfigured).unwrap();
        sm.apply(GameEvent::Pause(PauseKind::Buzz {
            id: "deadbeef0002".into(),
        }))
        .unwrap();

        assert_eq!(
            sm.apply(GameEvent::Reveal).unwrap(),
            GamePhase::GameRunning(GameRunningPhase::Reveal)
        );
    }
}
