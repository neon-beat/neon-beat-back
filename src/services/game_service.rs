use std::collections::HashSet;

use dashmap::DashMap;
use uuid::Uuid;

use crate::{
    dao::{
        game::GameRepository,
        models::{GameEntity, PlaylistEntity},
    },
    dto::game::{CreateGameRequest, GameSummary, PlayerInput, PlaylistInput, SongInput},
    error::ServiceError,
    state::{
        self, SharedState,
        game::{GameSession, Player, Playlist, PointField, Song},
    },
};

const BUZZER_ID_LENGTH: usize = 12;

/// Bootstrap a fresh game during the idle state.
pub async fn create_game(
    state: &SharedState,
    request: CreateGameRequest,
) -> Result<GameSummary, ServiceError> {
    ensure_idle(state).await?;

    let game = build_game_session(request)?;

    let repository = GameRepository::new(state.mongo());
    repository.save(game.clone().into()).await?;

    {
        let mut slot = state.current_game().write().await;
        *slot = Some(game.clone());
    }

    Ok(game.into())
}

/// Load an existing game from the database into the shared state.
pub async fn load_game(state: &SharedState, id: Uuid) -> Result<GameSummary, ServiceError> {
    ensure_idle(state).await?;

    let repository = GameRepository::new(state.mongo());
    let Some(game) = repository.find(id).await? else {
        return Err(ServiceError::NotFound(format!("game `{id}` not found")));
    };
    let Some(playlist) = repository.find_playlist(game.playlist_id).await? else {
        return Err(ServiceError::NotFound(format!(
            "playlist `{}` not found",
            game.playlist_id
        )));
    };

    validate_persisted_game(&game, &playlist)?;

    let game_session: GameSession = (game, playlist).into();
    {
        let mut slot = state.current_game().write().await;
        *slot = Some(game_session.clone());
    }

    Ok(game_session.into())
}

async fn ensure_idle(state: &SharedState) -> Result<(), ServiceError> {
    let guard = state.game().read().await;
    if !matches!(guard.phase(), state::state_machine::GamePhase::Idle) {
        return Err(ServiceError::InvalidState(
            "game can only be bootstrapped while idle".into(),
        ));
    }
    Ok(())
}

fn build_game_session(request: CreateGameRequest) -> Result<GameSession, ServiceError> {
    let CreateGameRequest {
        name,
        players,
        playlist,
    } = request;

    if name.trim().is_empty() {
        return Err(ServiceError::InvalidInput(
            "game name must not be empty".into(),
        ));
    }

    if players.is_empty() {
        return Err(ServiceError::InvalidInput(
            "a game requires at least one player".into(),
        ));
    }

    let PlaylistInput {
        name: playlist_name,
        songs,
    } = playlist;

    if songs.is_empty() {
        return Err(ServiceError::InvalidInput(
            "playlist must contain at least one song".into(),
        ));
    }

    let players = build_players(players)?;
    let playlist = build_playlist(songs, playlist_name)?;

    Ok(GameSession::new(name, players, playlist))
}

fn build_players(players: Vec<PlayerInput>) -> Result<Vec<Player>, ServiceError> {
    let mut seen_ids = HashSet::new();
    players
        .into_iter()
        .map(|player| {
            let mut buzzer_id = player.buzzer_id.to_lowercase();
            buzzer_id.retain(|c| !c.is_whitespace());

            if !is_valid_buzzer_id(&buzzer_id) {
                return Err(ServiceError::InvalidInput(format!(
                    "invalid buzzer id `{}`: expected {} lowercase hex characters",
                    player.buzzer_id, BUZZER_ID_LENGTH
                )));
            }

            if !seen_ids.insert(buzzer_id.clone()) {
                return Err(ServiceError::InvalidInput(format!(
                    "duplicate buzzer id `{}` detected",
                    buzzer_id
                )));
            }

            if player.name.trim().is_empty() {
                return Err(ServiceError::InvalidInput(
                    "player name must not be empty".into(),
                ));
            }

            Ok(Player {
                buzzer_id,
                name: player.name,
                score: 0,
            })
        })
        .collect()
}

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
                    start_time_ms: song.starts_at_ms,
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
        .collect::<Result<DashMap<u32, Song>, ServiceError>>()?;

    Ok(Playlist::new(name, songs))
}

fn validate_persisted_game(
    game: &GameEntity,
    playlist: &PlaylistEntity,
) -> Result<(), ServiceError> {
    if game.players.is_empty() {
        return Err(ServiceError::InvalidState(format!(
            "game `{}` has no registered players",
            game.id
        )));
    }

    if playlist.songs.is_empty() {
        return Err(ServiceError::InvalidState(format!(
            "game `{}` has an empty playlist",
            game.id
        )));
    }

    let expected = playlist.songs.len();
    let song_order = &game.playlist_song_order;
    if song_order.len() != expected {
        return Err(ServiceError::InvalidState(format!(
            "game `{}` song orger is inconsistent (expected {} entries, got {})",
            game.id,
            expected,
            song_order.len()
        )));
    }

    let song_ids = playlist.songs.keys().collect::<HashSet<_>>();
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
