use std::{collections::HashSet, time::SystemTime};

use indexmap::IndexMap;
use rand::{rng, seq::SliceRandom};
use uuid::Uuid;

use crate::{
    config::AppConfig,
    dao::models::{GameEntity, PlaylistEntity},
    dto::game::{GameSummary, PlaylistInput, PlaylistSummary, SongInput, TeamInput},
    error::ServiceError,
    services::sse_events,
    state::{
        self, SharedState,
        game::{GameSession, Playlist, PointField, Song, Team},
    },
};

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
    let store = state.require_game_store().await?;
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
    shuffle_playlist: bool,
) -> Result<GameSummary, ServiceError> {
    ensure_idle(state).await?;
    let config = state.config();

    if name.trim().is_empty() {
        return Err(ServiceError::InvalidInput(
            "game name must not be empty".into(),
        ));
    }

    let teams = build_teams(teams, config.as_ref())?;

    let playlist = match playlist {
        Some(p) => p,
        None => {
            let store = state.require_game_store().await?;
            let playlist_entity = store.find_playlist(playlist_id).await?.ok_or_else(|| {
                ServiceError::NotFound(format!("playlist `{}` not found", playlist_id))
            })?;
            playlist_entity.into()
        }
    };

    if playlist.songs.is_empty() {
        return Err(ServiceError::InvalidInput(
            "playlist must contain at least one song".into(),
        ));
    }

    let game = GameSession::new(name, teams, playlist, shuffle_playlist);
    if game.playlist_song_order.is_empty() {
        panic!("playlist_song_order should not be empty")
    };

    state
        .with_current_game_slot_mut(|slot| {
            *slot = Some(game.clone());
        })
        .await;

    // Clear all game-scoped state from previous game
    state.clear_game_state().await;

    state.persist_current_game().await?;

    sse_events::broadcast_game_session(state, &game);

    Ok(game.into())
}

/// Load an existing game from the database into the shared state.
pub async fn load_game(
    state: &SharedState,
    id: Uuid,
    shuffle_playlist: bool,
) -> Result<GameSummary, ServiceError> {
    ensure_idle(state).await?;

    let store = state.require_game_store().await?;

    let Some(game) = store.find_game(id).await? else {
        return Err(ServiceError::NotFound(format!("game `{id}` not found")));
    };

    if game.playlist_song_order.is_empty() {
        panic!("playlist_song_order should not be empty")
    };

    let current_song_index = game.current_song_index;
    let current_song_found = game.current_song_found;
    let is_playlist_in_progress = if let Some(current_song_index) = current_song_index {
        if current_song_found && current_song_index >= game.playlist_song_order.len() - 1 {
            // Playlist was completed in the previous session
            false
        } else if !current_song_found && current_song_index == 0 {
            // Playlist has not been started in the previous session
            false
        } else {
            // Playlist is in progress
            true
        }
    } else {
        // Playlist was completed in the previous session
        false
    };
    if shuffle_playlist && is_playlist_in_progress {
        return Err(ServiceError::InvalidInput(
            "shuffle parameter cannot be used: game is already in progress".into(),
        ));
    }

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

    validate_persisted_game(&game, &playlist)?;

    let mut game_session: GameSession = (game, playlist).into();

    if shuffle_playlist {
        let mut rng = rng();
        game_session.playlist_song_order.shuffle(&mut rng);
        game_session.updated_at = SystemTime::now();
    };

    state
        .with_current_game_slot_mut(|slot| {
            *slot = Some(game_session.clone());
        })
        .await;

    // Clear all game-scoped state from previous game
    state.clear_game_state().await;

    if shuffle_playlist {
        state.persist_current_game_without_teams().await?;
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

/// Validate incoming DTO teams, applying defaults and allocating a color from the colors set when
/// none is provided. Ensures buzzer IDs remain unique.
fn build_teams(
    teams: Vec<TeamInput>,
    config: &AppConfig,
) -> Result<IndexMap<Uuid, Team>, ServiceError> {
    let mut seen_ids = HashSet::new();
    let mut used_colors = Vec::new();

    teams
        .into_iter()
        .map(|team| {
            let buzzer_id = team
                .buzzer_id
                .unwrap_or_default()
                .as_ref()
                .map(|id| {
                    if !seen_ids.insert(id.clone()) {
                        Err(ServiceError::InvalidInput(format!(
                            "duplicate buzzer id `{}` detected",
                            id
                        )))
                    } else {
                        Ok(id.clone())
                    }
                })
                .transpose()?;

            if team.name.trim().is_empty() {
                return Err(ServiceError::InvalidInput(
                    "team name must not be empty".into(),
                ));
            }

            // Pick the first free color; fall back to the colors set order if everything is taken.
            let color = team
                .color
                .map(Into::into)
                .unwrap_or_else(|| config.first_unused_color(&used_colors));
            used_colors.push(color.clone());

            let team = Team {
                buzzer_id,
                name: team.name,
                score: team.score.unwrap_or_default(),
                color,
                updated_at: SystemTime::now(),
            };

            Ok((Uuid::new_v4(), team))
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
