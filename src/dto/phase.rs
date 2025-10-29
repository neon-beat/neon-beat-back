use serde::Serialize;
use utoipa::ToSchema;

use crate::state::state_machine::{GamePhase, GameRunningPhase, PrepStatus};

/// Publicly visible game phase exposed to clients (REST/SSE).
#[derive(Debug, Serialize, ToSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum VisibleGamePhase {
    /// No active game.
    Idle,
    /// Game in prep phase, ready to start.
    PrepReady,
    /// Game in prep phase, pairing buzzers with teams.
    PrepPairing,
    /// Game is actively playing.
    Playing,
    /// Game is paused (manual or buzz).
    Pause,
    /// Revealing the answer for the current song.
    Reveal,
    /// Showing final scores.
    Scores,
}

impl From<&GamePhase> for VisibleGamePhase {
    fn from(value: &GamePhase) -> Self {
        match value {
            GamePhase::Idle => VisibleGamePhase::Idle,
            GamePhase::ShowScores => VisibleGamePhase::Scores,
            GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Ready)) => {
                VisibleGamePhase::PrepReady
            }
            GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Pairing(_))) => {
                VisibleGamePhase::PrepPairing
            }
            GamePhase::GameRunning(GameRunningPhase::Playing) => VisibleGamePhase::Playing,
            GamePhase::GameRunning(GameRunningPhase::Paused(_)) => VisibleGamePhase::Pause,
            GamePhase::GameRunning(GameRunningPhase::Reveal) => VisibleGamePhase::Reveal,
        }
    }
}
