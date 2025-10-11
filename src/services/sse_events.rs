use serde::Serialize;
use tracing::warn;

use crate::{
    dto::sse::{
        AnswerValidationEvent, FieldsFoundEvent, PhaseChangedEvent, PhaseSnapshot,
        PointFieldSnapshot, ServerEvent, SongSnapshot, TeamSummary, TeamsEvent,
    },
    state::{
        SharedState,
        game::{GameSession, Player},
        state_machine::{GamePhase, GameRunningPhase, PauseKind},
    },
};

const EVENT_GAME_TEAMS: &str = "game_teams";
const EVENT_FIELDS_FOUND: &str = "fields_found";
const EVENT_ANSWER_VALIDATION: &str = "answer_validation";
const EVENT_SCORE_ADJUSTMENT: &str = "score_adjustment";
const EVENT_PHASE_CHANGED: &str = "phase_changed";

/// Broadcast the list of teams to public subscribers (game created or loaded).
pub fn broadcast_game_teams(state: &SharedState, teams: &[Player]) {
    let payload = TeamsEvent {
        teams: players_to_summaries(teams),
    };
    send_public_event(state, EVENT_GAME_TEAMS, &payload);
    send_admin_event(state, EVENT_GAME_TEAMS, &payload);
}

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
    send_admin_event(state, EVENT_FIELDS_FOUND, &payload);
}

/// Broadcast whether the current answer has been validated or invalidated.
pub fn broadcast_answer_validation(state: &SharedState, valid: bool) {
    let payload = AnswerValidationEvent { valid };
    send_public_event(state, EVENT_ANSWER_VALIDATION, &payload);
    send_admin_event(state, EVENT_ANSWER_VALIDATION, &payload);
}

/// Broadcast a score adjustment for a specific team.
pub fn broadcast_score_adjustment(state: &SharedState, team: Player) {
    let payload = TeamSummary::from(team);
    send_public_event(state, EVENT_SCORE_ADJUSTMENT, &payload);
    send_admin_event(state, EVENT_SCORE_ADJUSTMENT, &payload);
}

/// Broadcast a gameplay phase change notification.
pub async fn broadcast_phase_changed(state: &SharedState, phase: &GamePhase) {
    if let Some(snapshot) = build_phase_changed_event(state, phase).await {
        send_public_event(state, EVENT_PHASE_CHANGED, &snapshot);
        send_admin_event(state, EVENT_PHASE_CHANGED, &snapshot);
    }
}

fn players_to_summaries(players: &[Player]) -> Vec<TeamSummary> {
    players.iter().cloned().map(TeamSummary::from).collect()
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
    let kind = phase_kind(phase);
    let paused_buzzer = match phase {
        GamePhase::GameRunning(GameRunningPhase::Paused(PauseKind::Buzz { id })) => {
            Some(id.clone())
        }
        _ => None,
    };

    let (song, scoreboard) = {
        let guard = state.current_game().read().await;
        match guard.as_ref() {
            Some(game) => (
                song_snapshot_for_phase(game, phase),
                scoreboard_for_phase(game, phase),
            ),
            None => (None, None),
        }
    };

    Some(PhaseChangedEvent {
        phase: PhaseSnapshot { kind },
        song,
        scoreboard,
        paused_buzzer,
    })
}

fn phase_kind(phase: &GamePhase) -> String {
    match phase {
        GamePhase::Idle => "idle",
        GamePhase::ShowScores => "scores",
        GamePhase::GameRunning(GameRunningPhase::Prep) => "prep",
        GamePhase::GameRunning(GameRunningPhase::Playing) => "playing",
        GamePhase::GameRunning(GameRunningPhase::Paused(_)) => "pause",
        GamePhase::GameRunning(GameRunningPhase::Reveal) => "reveal",
    }
    .to_string()
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
        GamePhase::ShowScores => Some(players_to_summaries(&game.players)),
        _ => None,
    }
}

fn current_song_snapshot(game: &GameSession) -> Option<SongSnapshot> {
    let index = game.current_song_index?;
    let song_id = *game.playlist_song_order.get(index)?;
    let song_ref = game.playlist.songs.get(&song_id)?;
    let song = song_ref.value();

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
