//! Service helpers that expose read-only public projections of the current game.

use crate::{
    dto::{
        public::{CurrentSongResponse, GamePhaseResponse, PairingStatusResponse, TeamsResponse},
        sse::TeamSummary,
    },
    error::ServiceError,
    state::{
        SharedState,
        state_machine::{GamePhase, GameRunningPhase, PrepStatus},
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
        phase: (&phase).into(),
        degraded,
    })
}

/// Return the current pairing workflow status for public consumers.
pub async fn get_pairing_status(
    state: &SharedState,
) -> Result<PairingStatusResponse, ServiceError> {
    match state.state_machine_phase().await {
        GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Ready)) => {
            Ok(PairingStatusResponse {
                is_pairing: false,
                team_id: None,
            })
        }
        GamePhase::GameRunning(GameRunningPhase::Prep(PrepStatus::Pairing(session))) => {
            Ok(PairingStatusResponse {
                is_pairing: true,
                team_id: Some(session.pairing_team_id),
            })
        }
        _ => Ok(PairingStatusResponse {
            is_pairing: false,
            team_id: None,
        }),
    }
}
