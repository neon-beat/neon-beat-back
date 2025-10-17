use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::dto::{game::SongSummary, phase::VisibleGamePhase, sse::TeamSummary};

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

/// Response exposing the game's global phase as seen by the public.
#[derive(Debug, Serialize, ToSchema)]
pub struct GamePhaseResponse {
    pub phase: VisibleGamePhase,
    pub degraded: bool,
}

/// Public response describing the state of the pairing workflow.
#[derive(Debug, Serialize, ToSchema)]
pub struct PairingStatusResponse {
    pub is_pairing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<Uuid>,
}
