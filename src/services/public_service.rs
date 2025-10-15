//! Service helpers that expose read-only public projections of the current game.

use crate::{
    dto::{
        public::{CurrentSongResponse, GamePhaseResponse, PublicPhase, TeamsResponse},
        sse::TeamSummary,
    },
    error::ServiceError,
    state::{
        SharedState,
        state_machine::{GamePhase, GameRunningPhase},
    },
};

/// Return the current teams/players exposed to the public UI.
pub async fn get_teams(state: &SharedState) -> Result<TeamsResponse, ServiceError> {
    let guard = state.current_game().read().await;
    let game = guard
        .as_ref()
        .ok_or_else(|| ServiceError::NotFound("no active game".into()))?;

    let teams = game
        .players
        .clone()
        .into_iter()
        .map(TeamSummary::from)
        .collect();
    Ok(TeamsResponse { teams })
}

/// Return the song being played alongside any fields already discovered.
pub async fn get_current_song(state: &SharedState) -> Result<CurrentSongResponse, ServiceError> {
    let guard = state.current_game().read().await;
    let game = guard
        .as_ref()
        .ok_or_else(|| ServiceError::NotFound("no active game".into()))?;

    let index = game
        .current_song_index
        .ok_or_else(|| ServiceError::NotFound("no active song".into()))?;
    let (song_id, song) = game
        .get_song(index)
        .ok_or_else(|| ServiceError::InvalidState("song not found in playlist".into()))?;

    let song_summary = (song_id, song).into();
    Ok(CurrentSongResponse {
        song: song_summary,
        found_point_fields: game.found_point_fields.clone(),
        found_bonus_fields: game.found_bonus_fields.clone(),
    })
}

/// Return the current game phase (e.g. idle, playing, reveal) and degraded mode.
pub async fn get_game_phase(state: &SharedState) -> Result<GamePhaseResponse, ServiceError> {
    let phase = state.state_machine_phase().await;
    let degraded = state.is_degraded().await;

    Ok(GamePhaseResponse {
        phase: map_phase(&phase),
        degraded,
    })
}

/// Translate the internal state-machine phase into the public snapshot enum.
fn map_phase(value: &GamePhase) -> PublicPhase {
    match value {
        GamePhase::Idle => PublicPhase::Idle,
        GamePhase::ShowScores => PublicPhase::Scores,
        GamePhase::GameRunning(GameRunningPhase::Prep) => PublicPhase::Prep,
        GamePhase::GameRunning(GameRunningPhase::Playing) => PublicPhase::Playing,
        GamePhase::GameRunning(GameRunningPhase::Paused(_)) => PublicPhase::Pause,
        GamePhase::GameRunning(GameRunningPhase::Reveal) => PublicPhase::Reveal,
    }
}
