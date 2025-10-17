use uuid::Uuid;

use crate::{
    error::ServiceError,
    services::sse_events,
    state::{
        SharedState,
        game::Player,
        state_machine::{GameEvent, PairingSession},
        transitions::run_transition_with_broadcast,
    },
};

/// Result of driving the pairing workflow forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingProgress {
    Wait(Uuid),
    Finish,
}

/// Mutation the state machine pairing session should apply after a roster change.
#[derive(Debug)]
pub enum PairingSessionUpdate {
    Assigned { team_id: Uuid, roster: Vec<Player> },
    Deleted { team_id: Uuid, roster: Vec<Player> },
}

/// Return the identifier of the next team without a buzzer assigned, if any.
fn next_unassigned_team(players: &[Player]) -> Option<Uuid> {
    players
        .iter()
        .find(|player| player.buzzer_id.is_none())
        .map(|player| player.id)
}

/// Advance the pairing workflow, updating the session state and describing the outcome.
fn advance_pairing(session: &mut PairingSession, players: &[Player]) -> PairingProgress {
    if let Some(next) = next_unassigned_team(players) {
        session.pairing_team_id = next;
        PairingProgress::Wait(next)
    } else {
        PairingProgress::Finish
    }
}

/// Apply an update to the pairing session captured inside the state machine.
///
/// The provided roster should reflect the latest in-memory players list for the active game.
/// Returns `Ok(Some(progress))` when the pairing target also changed (so callers must react),
/// `Ok(None)` when the update is unrelated to the current pairing team, and `Err` if no session
/// is active.
pub async fn apply_pairing_update(
    state: &SharedState,
    update: PairingSessionUpdate,
) -> Result<Option<PairingProgress>, ServiceError> {
    state
        .with_pairing_session_mut(|session| {
            let (team_id, roster) = match update {
                PairingSessionUpdate::Assigned { team_id, roster } => (team_id, roster),
                PairingSessionUpdate::Deleted { team_id, roster } => {
                    session.snapshot.retain(|player| player.id != team_id);
                    (team_id, roster)
                }
            };
            if session.pairing_team_id == team_id {
                Some(advance_pairing(session, &roster))
            } else {
                None
            }
        })
        .await
}

/// React to the outcome of `advance_pairing`, emitting SSE updates and optionally
/// triggering additional work when pairing is complete.
pub async fn handle_pairing_progress(
    state: &SharedState,
    progress: PairingProgress,
) -> Result<(), ServiceError> {
    match progress {
        PairingProgress::Wait(team_id) => {
            sse_events::broadcast_pairing_waiting(state, team_id);
            Ok(())
        }
        PairingProgress::Finish => {
            run_transition_with_broadcast(state, GameEvent::PairingFinished, move || async move {
                Ok(())
            })
            .await
        }
    }
}
