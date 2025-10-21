use std::collections::HashSet;

use indexmap::IndexMap;
use uuid::Uuid;

use crate::{
    dao::models::{GameEntity, PlaylistEntity},
    dto::game::{GameSummary, PlaylistInput, PlaylistSummary, SongInput, TeamInput},
    error::ServiceError,
    services::sse_events,
    state::{
        self, SharedState,
        game::{GameSession, Playlist, PointField, Song, Team},
    },
};

const BUZZER_ID_LENGTH: usize = 12;

/// Create and persist a reusable playlist definition on behalf of admins.
pub async fn create_playlist(
    state: &SharedState,
    request: PlaylistInput,
) -> Result<(PlaylistSummary, Playlist), ServiceError> {
    let PlaylistInput { name, songs } = request;
    tracing::warn!("SONGS: {:?}", songs);

    if songs.is_empty() {
        return Err(ServiceError::InvalidInput(
            "playlist songs must not be empty".into(),
        ));
    }

    let playlist = build_playlist(songs, name)?;
    tracing::warn!("PLAYLIST: {:?}", playlist);

    // Preserve deterministic ordering based on the assigned song identifiers.
    let song_count = playlist.songs.len() as u32;
    let order: Vec<u32> = (0..song_count).collect();
    let summary: PlaylistSummary = (playlist.clone(), order).into();
    tracing::warn!("SUMMARY: {:?}", playlist);

    let entity: PlaylistEntity = playlist.clone().into();
    tracing::warn!("ENTITY: {:?}", playlist);
    let store = state.game_store().await.ok_or(ServiceError::Degraded)?;
    store.save_playlist(entity).await?;

    Ok((summary, playlist))
}

/// Bootstrap a fresh game during the idle state (with or without a playlist).
pub async fn create_game(
    state: &SharedState,
    name: String,
    teams: Vec<TeamInput>,
    playlist_id: Uuid,
    playlist: Option<Playlist>,
) -> Result<GameSummary, ServiceError> {
    ensure_idle(state).await?;

    if name.trim().is_empty() {
        return Err(ServiceError::InvalidInput(
            "game name must not be empty".into(),
        ));
    }

    let teams = if teams.is_empty() {
        IndexMap::new()
    } else {
        build_teams(teams)?
    };

    let store = state.game_store().await.ok_or(ServiceError::Degraded)?;

    let playlist = playlist.unwrap_or({
        let playlist_entity = store.find_playlist(playlist_id).await?.ok_or_else(|| {
            ServiceError::NotFound(format!("playlist `{}` not found", playlist_id))
        })?;
        playlist_entity.into()
    });

    if playlist.songs.is_empty() {
        return Err(ServiceError::InvalidInput(
            "playlist must contain at least one song".into(),
        ));
    }

    let game = GameSession::new(name, teams, playlist);
    if game.playlist_song_order.is_empty() {
        panic!("playlist_song_order should not be empty")
    };

    store.save_game(game.clone().into()).await?;
    {
        let mut slot = state.current_game().write().await;
        *slot = Some(game.clone());
    }

    sse_events::broadcast_game_session(state, &game);

    Ok(game.into())
}

/// Load an existing game from the database into the shared state.
pub async fn load_game(state: &SharedState, id: Uuid) -> Result<GameSummary, ServiceError> {
    ensure_idle(state).await?;

    let store = state.game_store().await.ok_or(ServiceError::Degraded)?;

    let Some(game) = store.find_game(id).await? else {
        return Err(ServiceError::NotFound(format!("game `{id}` not found")));
    };

    let Some(playlist) = store.find_playlist(game.playlist_id).await? else {
        return Err(ServiceError::NotFound(format!(
            "playlist `{}` not found",
            game.playlist_id
        )));
    };

    if playlist.songs.is_empty() {
        return Err(ServiceError::InvalidInput(
            "playlist must contain at least one song".into(),
        ));
    }
    if game.playlist_song_order.is_empty() {
        panic!("playlist_song_order should not be empty")
    };

    validate_persisted_game(&game, &playlist)?;

    let game_session: GameSession = (game, playlist).into();
    {
        let mut slot = state.current_game().write().await;
        *slot = Some(game_session.clone());
    }

    sse_events::broadcast_game_session(state, &game_session);

    Ok(game_session.into())
}

async fn ensure_idle(state: &SharedState) -> Result<(), ServiceError> {
    let phase = state.state_machine_phase().await;
    if !matches!(phase, state::state_machine::GamePhase::Idle) {
        return Err(ServiceError::InvalidState(
            "game can only be bootstrapped while idle".into(),
        ));
    }
    Ok(())
}

fn build_teams(teams: Vec<TeamInput>) -> Result<IndexMap<Uuid, Team>, ServiceError> {
    let mut seen_ids = HashSet::new();
    teams
        .into_iter()
        .map(|team| {
            let buzzer_id = team
                .buzzer_id
                .as_ref()
                .map(|id| sanitize_buzzer_id(id))
                .transpose()?
                .map(|id| {
                    if !seen_ids.insert(id.clone()) {
                        Err(ServiceError::InvalidInput(format!(
                            "duplicate buzzer id `{}` detected",
                            id
                        )))
                    } else {
                        Ok(id)
                    }
                })
                .transpose()?;

            if team.name.trim().is_empty() {
                return Err(ServiceError::InvalidInput(
                    "team name must not be empty".into(),
                ));
            }

            Ok((
                Uuid::new_v4(),
                Team {
                    buzzer_id,
                    name: team.name,
                    score: 0,
                },
            ))
        })
        .collect()
}

/// Construct a playlist from user-provided song metadata.
fn build_playlist(songs: Vec<SongInput>, name: String) -> Result<Playlist, ServiceError> {
    if name.trim().is_empty() {
        return Err(ServiceError::InvalidInput(
            "playlist name must not be empty".into(),
        ));
    }

    let songs = songs
        .into_iter()
        .enumerate()
        .map(|(index, song)| {
            if song.point_fields.is_empty() {
                return Err(ServiceError::InvalidInput(
                    "each song must declare at least one point field".into(),
                ));
            }

            if song.url.trim().is_empty() {
                return Err(ServiceError::InvalidInput(
                    "song url must not be empty".into(),
                ));
            }

            if song.guess_duration_ms == 0 {
                return Err(ServiceError::InvalidInput(
                    "guess duration must be strictly positive".into(),
                ));
            }

            Ok((
                (index as u32),
                Song {
                    starts_at_ms: song.starts_at_ms,
                    guess_duration_ms: song.guess_duration_ms,
                    url: song.url,
                    point_fields: song
                        .point_fields
                        .into_iter()
                        .map(|pf| PointField {
                            key: pf.key,
                            value: pf.value,
                            points: pf.points,
                        })
                        .collect(),
                    bonus_fields: song
                        .bonus_fields
                        .into_iter()
                        .map(|pf| PointField {
                            key: pf.key,
                            value: pf.value,
                            points: pf.points,
                        })
                        .collect(),
                },
            ))
        })
        .collect::<Result<IndexMap<u32, Song>, ServiceError>>()?;

    Ok(Playlist::new(name, songs))
}

fn validate_persisted_game(
    game: &GameEntity,
    playlist: &PlaylistEntity,
) -> Result<(), ServiceError> {
    if game.teams.is_empty() {
        return Err(ServiceError::InvalidState(format!(
            "game `{}` has no registered teams",
            game.id
        )));
    }

    if playlist.songs.is_empty() {
        return Err(ServiceError::InvalidState(format!(
            "game `{}` has an empty playlist",
            game.id
        )));
    }

    let playlist_songs_nb = playlist.songs.len();
    let song_order = &game.playlist_song_order;
    if song_order.len() != playlist_songs_nb {
        return Err(ServiceError::InvalidState(format!(
            "game `{}` song orger is inconsistent (expected {} entries, got {})",
            game.id,
            playlist_songs_nb,
            song_order.len()
        )));
    }

    let song_ids = (0..playlist_songs_nb as u32).collect::<HashSet<_>>();
    for song_id in song_order {
        if !song_ids.contains(song_id) {
            return Err(ServiceError::InvalidState(format!(
                "game `{}` song orger references unknown song `{}`",
                game.id, song_id
            )));
        }
    }

    Ok(())
}

fn is_valid_buzzer_id(value: &str) -> bool {
    value.len() == BUZZER_ID_LENGTH && value.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
}

/// Normalise and validate a buzzer identifier (lowercase hex, no whitespace).
pub fn sanitize_buzzer_id(raw: &str) -> Result<String, ServiceError> {
    let mut buzzer_id = raw.to_lowercase();
    buzzer_id.retain(|c| !c.is_whitespace());

    if !is_valid_buzzer_id(&buzzer_id) {
        return Err(ServiceError::InvalidInput(format!(
            "invalid buzzer id `{}`: expected {} lowercase hex characters",
            raw, BUZZER_ID_LENGTH
        )));
    }

    Ok(buzzer_id)
}
