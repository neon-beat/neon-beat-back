use serde::Serialize;
use utoipa::ToSchema;

use crate::state::state_machine::{GamePhase, GameRunningPhase, PrepStatus};

/// Publicly visible game phase exposed to clients (REST/SSE).
#[derive(Debug, Serialize, ToSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum VisibleGamePhase {
    Idle,
    PrepReady,
    PrepPairing,
    Playing,
    Pause,
    Reveal,
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
