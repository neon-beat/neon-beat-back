use indexmap::IndexMap;
use serde::Serialize;
use tracing::warn;
use uuid::Uuid;

use crate::{
    dto::{
        admin::AnswerValidation,
        game::GameSummary,
        sse::{
            AnswerValidationEvent, FieldsFoundEvent, PairingAssignedEvent, PairingRestoredEvent,
            PairingWaitingEvent, PhaseChangedEvent, PointFieldSnapshot, ServerEvent, SongSnapshot,
            TeamCreatedEvent, TeamDeletedEvent, TeamSummary, TeamUpdatedEvent, TestBuzzEvent,
        },
    },
    state::{
        SharedState,
        game::{GameSession, Team},
        state_machine::{GamePhase, GameRunningPhase, PauseKind},
    },
};

const EVENT_FIELDS_FOUND: &str = "fields_found";
const EVENT_ANSWER_VALIDATION: &str = "answer_validation";
const EVENT_SCORE_ADJUSTMENT: &str = "score_adjustment";
const EVENT_PHASE_CHANGED: &str = "phase_changed";
const EVENT_TEAM_CREATED: &str = "team.created";
const EVENT_TEAM_UPDATED: &str = "team.updated";
const EVENT_PAIRING_WAITING: &str = "pairing.waiting";
const EVENT_PAIRING_ASSIGNED: &str = "pairing.assigned";
const EVENT_PAIRING_RESTORED: &str = "pairing.restored";
const EVENT_TEST_BUZZ: &str = "test.buzz";
const EVENT_TEAM_DELETED: &str = "team.deleted";
const EVENT_GAME_SESSION: &str = "game.session";

/// Broadcast the list of fields found for the current song.
pub fn broadcast_fields_found(
    state: &SharedState,
    song_id: u32,
    point_fields: &[String],
    bonus_fields: &[String],
) {
    let payload = FieldsFoundEvent {
        song_id,
        point_fields: point_fields.to_vec(),
        bonus_fields: bonus_fields.to_vec(),
    };
    send_public_event(state, EVENT_FIELDS_FOUND, &payload);
}

/// Broadcast whether the current answer has been validated or invalidated.
pub fn broadcast_answer_validation(state: &SharedState, valid: AnswerValidation) {
    let payload = AnswerValidationEvent { valid };
    send_public_event(state, EVENT_ANSWER_VALIDATION, &payload);
}

/// Broadcast a score adjustment for a specific team.
pub fn broadcast_score_adjustment(state: &SharedState, team_id: Uuid, team: Team) {
    let payload = TeamSummary::from((team_id, team));
    send_public_event(state, EVENT_SCORE_ADJUSTMENT, &payload);
}

/// Broadcast the creation of a new team to admins.
pub fn broadcast_team_created(state: &SharedState, team: TeamSummary) {
    let payload = TeamCreatedEvent { team };
    send_public_event(state, EVENT_TEAM_CREATED, &payload);
    send_admin_event(state, EVENT_TEAM_CREATED, &payload);
}

/// Broadcast that a team has been deleted to public subscribers.
pub fn broadcast_team_deleted(state: &SharedState, team_id: Uuid) {
    let payload = TeamDeletedEvent { team_id };
    send_public_event(state, EVENT_TEAM_DELETED, &payload);
}

/// Broadcast that a team has been updated to public subscribers.
pub fn broadcast_team_updated(state: &SharedState, team: TeamSummary) {
    let payload = TeamUpdatedEvent { team };
    send_public_event(state, EVENT_TEAM_UPDATED, &payload);
}

/// Broadcast a snapshot of the entire game session to public subscribers.
pub fn broadcast_game_session(state: &SharedState, session: &GameSession) {
    let summary: GameSummary = session.clone().into();
    send_public_event(state, EVENT_GAME_SESSION, &summary);
}

/// Broadcast that the pairing workflow is waiting for the specified team.
pub fn broadcast_pairing_waiting(state: &SharedState, team_id: Uuid) {
    let payload = PairingWaitingEvent { team_id };
    send_public_event(state, EVENT_PAIRING_WAITING, &payload);
    send_admin_event(state, EVENT_PAIRING_WAITING, &payload);
}

/// Broadcast that a buzzer has been assigned during pairing.
pub fn broadcast_pairing_assigned(state: &SharedState, team_id: Uuid, buzzer_id: &str) {
    let payload = PairingAssignedEvent {
        team_id,
        buzzer_id: buzzer_id.to_string(),
    };
    send_public_event(state, EVENT_PAIRING_ASSIGNED, &payload);
    send_admin_event(state, EVENT_PAIRING_ASSIGNED, &payload);
}

/// Broadcast that pairing snapshot was restored.
pub fn broadcast_pairing_restored(state: &SharedState, snapshot: IndexMap<Uuid, Team>) {
    let payload = PairingRestoredEvent {
        snapshot: snapshot.into_iter().map(TeamSummary::from).collect(),
    };
    send_public_event(state, EVENT_PAIRING_RESTORED, &payload);
}

/// Broadcast a test buzz event during prep ready mode.
pub fn broadcast_test_buzz(state: &SharedState, team_id: Uuid) {
    let payload = TestBuzzEvent { team_id };
    send_public_event(state, EVENT_TEST_BUZZ, &payload);
    send_admin_event(state, EVENT_TEST_BUZZ, &payload);
}

/// Broadcast a gameplay phase change notification.
pub async fn broadcast_phase_changed(state: &SharedState, phase: &GamePhase) {
    if let Some(snapshot) = build_phase_changed_event(state, phase).await {
        send_public_event(state, EVENT_PHASE_CHANGED, &snapshot);
        send_admin_event(state, EVENT_PHASE_CHANGED, &snapshot);
    }
}

fn teams_to_summaries(teams: IndexMap<Uuid, Team>) -> Vec<TeamSummary> {
    teams.into_iter().map(Into::into).collect()
}

fn send_public_event(state: &SharedState, event: &str, payload: &impl Serialize) {
    match ServerEvent::json(Some(event.to_string()), payload) {
        Ok(event) => state.public_sse().broadcast(event),
        Err(err) => warn!(event, error = %err, "failed to serialize public SSE payload"),
    }
}

fn send_admin_event(state: &SharedState, event: &str, payload: &impl Serialize) {
    match ServerEvent::json(Some(event.to_string()), payload) {
        Ok(event) => state.admin_sse().broadcast(event),
        Err(err) => warn!(event, error = %err, "failed to serialize admin SSE payload"),
    }
}

async fn build_phase_changed_event(
    state: &SharedState,
    phase: &GamePhase,
) -> Option<PhaseChangedEvent> {
    let kind = phase.into();
    let paused_buzzer = match phase {
        GamePhase::GameRunning(GameRunningPhase::Paused(PauseKind::Buzz { id })) => {
            Some(id.clone())
        }
        _ => None,
    };

    let (song, scoreboard) = state
        .read_current_game(|maybe| match maybe {
            Some(game) => (
                song_snapshot_for_phase(game, phase),
                scoreboard_for_phase(game, phase),
            ),
            None => (None, None),
        })
        .await;

    Some(PhaseChangedEvent {
        phase: kind,
        song,
        scoreboard,
        paused_buzzer,
    })
}

fn song_snapshot_for_phase(game: &GameSession, phase: &GamePhase) -> Option<SongSnapshot> {
    match phase {
        GamePhase::GameRunning(GameRunningPhase::Playing)
        | GamePhase::GameRunning(GameRunningPhase::Paused(_))
        | GamePhase::GameRunning(GameRunningPhase::Reveal) => current_song_snapshot(game),
        _ => None,
    }
}

fn scoreboard_for_phase(game: &GameSession, phase: &GamePhase) -> Option<Vec<TeamSummary>> {
    match phase {
        GamePhase::ShowScores => Some(teams_to_summaries(game.teams.clone())),
        _ => None,
    }
}

fn current_song_snapshot(game: &GameSession) -> Option<SongSnapshot> {
    let index = game.current_song_index?;
    let song_id = *game.playlist_song_order.get(index)?;
    let song = game.playlist.songs.get(&song_id)?;

    Some(SongSnapshot {
        id: song_id,
        starts_at_ms: song.starts_at_ms,
        guess_duration_ms: song.guess_duration_ms,
        url: song.url.clone(),
        point_fields: song
            .point_fields
            .iter()
            .cloned()
            .map(PointFieldSnapshot::from)
            .collect(),
        bonus_fields: song
            .bonus_fields
            .iter()
            .cloned()
            .map(PointFieldSnapshot::from)
            .collect(),
    })
}
