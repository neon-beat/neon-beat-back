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

/// Side-effects the caller needs to perform after a successful transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionEffect {
    NotifyBuzzerTurnEnded { buzzer_id: String },
}

/// Result of applying a valid transition on the state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateTransition {
    pub next: GamePhase,
    pub effects: Vec<TransitionEffect>,
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
    pub fn apply(&mut self, event: GameEvent) -> Result<StateTransition, InvalidTransition> {
        let (next, effects) = match (self.phase.clone(), event) {
            (GamePhase::Idle, GameEvent::StartGame) => {
                (GamePhase::GameRunning(GameRunningPhase::Prep), Vec::new())
            }
            (GamePhase::GameRunning(GameRunningPhase::Prep), GameEvent::GameConfigured) => (
                GamePhase::GameRunning(GameRunningPhase::Playing),
                Vec::new(),
            ),
            (GamePhase::GameRunning(GameRunningPhase::Playing), GameEvent::Pause(kind)) => (
                GamePhase::GameRunning(GameRunningPhase::Paused(kind)),
                Vec::new(),
            ),
            (GamePhase::GameRunning(GameRunningPhase::Playing), GameEvent::Reveal) => {
                (GamePhase::GameRunning(GameRunningPhase::Reveal), Vec::new())
            }
            (
                GamePhase::GameRunning(GameRunningPhase::Paused(kind)),
                GameEvent::ContinuePlaying,
            ) => {
                let mut effects = Vec::new();
                if let PauseKind::Buzz { id } = kind {
                    effects.push(TransitionEffect::NotifyBuzzerTurnEnded { buzzer_id: id });
                }
                (GamePhase::GameRunning(GameRunningPhase::Playing), effects)
            }
            (GamePhase::GameRunning(GameRunningPhase::Paused(kind)), GameEvent::Reveal) => {
                let mut effects = Vec::new();
                if let PauseKind::Buzz { id } = kind {
                    effects.push(TransitionEffect::NotifyBuzzerTurnEnded { buzzer_id: id });
                }
                (GamePhase::GameRunning(GameRunningPhase::Reveal), effects)
            }
            (GamePhase::GameRunning(GameRunningPhase::Reveal), GameEvent::NextSong) => (
                GamePhase::GameRunning(GameRunningPhase::Playing),
                Vec::new(),
            ),
            (GamePhase::GameRunning(_), GameEvent::Finish(reason)) => {
                self.last_finish_reason = Some(reason);
                (GamePhase::ShowScores, Vec::new())
            }
            (GamePhase::ShowScores, GameEvent::EndGame) => {
                self.last_finish_reason = None;
                (GamePhase::Idle, Vec::new())
            }
            (from, event) => {
                return Err(InvalidTransition { from, event });
            }
        };

        self.phase = next.clone();
        Ok(StateTransition { next, effects })
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
            sm.apply(GameEvent::StartGame).unwrap().next,
            GamePhase::GameRunning(GameRunningPhase::Prep)
        );
        assert_eq!(
            sm.apply(GameEvent::GameConfigured).unwrap().next,
            GamePhase::GameRunning(GameRunningPhase::Playing)
        );
        assert_eq!(
            sm.apply(GameEvent::Pause(PauseKind::Manual)).unwrap().next,
            GamePhase::GameRunning(GameRunningPhase::Paused(PauseKind::Manual))
        );
        assert_eq!(
            sm.apply(GameEvent::Reveal).unwrap().next,
            GamePhase::GameRunning(GameRunningPhase::Reveal)
        );
        assert_eq!(
            sm.apply(GameEvent::NextSong).unwrap().next,
            GamePhase::GameRunning(GameRunningPhase::Playing)
        );
        assert_eq!(
            sm.apply(GameEvent::Finish(FinishReason::PlaylistCompleted))
                .unwrap()
                .next,
            GamePhase::ShowScores
        );
        assert_eq!(
            sm.last_finish_reason(),
            Some(FinishReason::PlaylistCompleted)
        );
        assert_eq!(sm.apply(GameEvent::EndGame).unwrap().next, GamePhase::Idle);
        assert_eq!(sm.last_finish_reason(), None);
    }

    #[test]
    fn buzzing_causes_pause() {
        let mut sm = GameStateMachine::new();
        sm.apply(GameEvent::StartGame).unwrap();
        sm.apply(GameEvent::GameConfigured).unwrap();

        let buzzer_id = "deadbeef0001";
        let transition = sm
            .apply(GameEvent::Pause(PauseKind::Buzz {
                id: buzzer_id.to_string(),
            }))
            .unwrap();

        match transition.next {
            GamePhase::GameRunning(GameRunningPhase::Paused(PauseKind::Buzz { id })) => {
                assert_eq!(id, buzzer_id);
            }
            other => panic!("expected pause with buzz id, got {other:?}"),
        }
        assert!(transition.effects.is_empty());
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
                .unwrap()
                .next,
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

        let transition = sm.apply(GameEvent::ContinuePlaying).unwrap();
        assert_eq!(
            transition.effects,
            vec![TransitionEffect::NotifyBuzzerTurnEnded {
                buzzer_id: "deadbeef0001".into()
            }]
        );
        assert_eq!(
            transition.next,
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

        let transition = sm.apply(GameEvent::Reveal).unwrap();
        assert_eq!(
            transition.effects,
            vec![TransitionEffect::NotifyBuzzerTurnEnded {
                buzzer_id: "deadbeef0002".into()
            }]
        );
        assert_eq!(
            transition.next,
            GamePhase::GameRunning(GameRunningPhase::Reveal)
        );
    }
}
