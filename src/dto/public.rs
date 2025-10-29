use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::dto::{
    common::GamePhaseSnapshot,
    game::{SongSummary, TeamSummary},
};

/// Response payload listing the teams currently loaded in memory.
#[derive(Debug, Serialize, ToSchema)]
pub struct TeamsResponse {
    /// List of teams in the active game.
    pub teams: Vec<TeamSummary>,
}

/// Response describing the song currently being played and progress made so far.
#[derive(Debug, Serialize, ToSchema)]
pub struct CurrentSongResponse {
    /// Details of the current song.
    pub song: SongSummary,
    /// Keys of point fields already found.
    pub found_point_fields: Vec<String>,
    /// Keys of bonus fields already found.
    pub found_bonus_fields: Vec<String>,
}

/// Response exposing the game's global phase as seen by the public.
#[derive(Debug, Serialize, ToSchema)]
#[serde(transparent)]
pub struct GamePhaseResponse(pub GamePhaseSnapshot);

/// Public response describing the state of the pairing workflow.
#[derive(Debug, Serialize, ToSchema)]
pub struct PairingStatusResponse {
    /// Whether pairing is currently active.
    pub is_pairing: bool,
    /// ID of the team currently pairing (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<Uuid>,
}
