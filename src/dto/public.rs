use serde::Serialize;
use utoipa::ToSchema;

use crate::dto::{game::SongSummary, sse::TeamSummary};

/// Response payload listing the teams currently loaded in memory.
#[derive(Debug, Serialize, ToSchema)]
pub struct TeamsResponse {
    pub teams: Vec<TeamSummary>,
}

/// Response describing the song currently being played and progress made so far.
#[derive(Debug, Serialize, ToSchema)]
pub struct CurrentSongResponse {
    pub song: SongSummary,
    pub found_point_fields: Vec<String>,
    pub found_bonus_fields: Vec<String>,
}

/// High-level game phase snapshot exposed to public consumers.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PublicPhase {
    /// No game is currently running.
    Idle,
    /// Game is configuring assets before play begins.
    Prep,
    /// Actively playing the current song.
    Playing,
    /// Gameplay is paused (manual pause or buzzing team).
    Pause,
    /// Current song is being revealed to players.
    Reveal,
    /// Final scores are displayed.
    Scores,
}

/// Response exposing the game's global phase as seen by the public.
#[derive(Debug, Serialize, ToSchema)]
pub struct GamePhaseResponse {
    pub phase: PublicPhase,
    pub degraded: bool,
}
