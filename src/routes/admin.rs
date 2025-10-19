use axum::{
    Json, Router,
    body::Body,
    extract::{Path, State},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post, put},
};
use uuid::Uuid;

use crate::{
    dto::{
        admin::{
            ActionResponse, AnswerValidationRequest, CreateGameRequest, CreateTeamRequest,
            FieldsFoundResponse, GameListItem, MarkFieldRequest, NextSongResponse,
            PlaylistListItem, ScoreAdjustmentRequest, ScoreUpdateResponse, StartGameResponse,
            StartPairingRequest, StopGameResponse, UpdateTeamRequest,
        },
        game::{CreateGameWithPlaylistRequest, GameSummary, PlaylistInput, PlaylistSummary},
        sse::TeamSummary,
    },
    error::AppError,
    services::admin_service,
    state::SharedState,
};

const ADMIN_TOKEN_HEADER: &str = "x-admin-token";

/// Admin-only management endpoints for configuring and driving games.
pub fn router(state: SharedState) -> Router<SharedState> {
    Router::new()
        .route("/admin/games", get(list_games).post(create_game))
        .route(
            "/admin/games/with-playlist",
            post(create_game_with_playlist),
        )
        .route("/admin/games/{id}", get(get_game_by_id).delete(delete_game))
        .route("/admin/games/{id}/load", post(load_game))
        .route(
            "/admin/playlists",
            get(list_playlists).post(create_playlist),
        )
        .route("/admin/game/start", post(start_game))
        .route("/admin/game/pause", post(pause_game))
        .route("/admin/game/resume", post(resume_game))
        .route("/admin/game/reveal", post(reveal_song))
        .route("/admin/game/next", post(next_song))
        .route("/admin/game/stop", post(stop_game))
        .route("/admin/game/end", post(end_game))
        .route("/admin/game/fields/found", post(mark_field_found))
        .route("/admin/game/answer", post(validate_answer))
        .route("/admin/game/score", post(adjust_score))
        .route("/admin/teams", post(create_team))
        .route("/admin/teams/{id}", put(update_team).delete(delete_team))
        .route("/admin/teams/pairing", post(start_pairing))
        .route("/admin/teams/pairing/abort", post(abort_pairing))
        .route_layer(middleware::from_fn_with_state(state, require_admin_token))
}

/// Retrieve all games known to the system for administration purposes.
#[utoipa::path(
    get,
    path = "/admin/games",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    responses((status = 200, description = "List available games", body = [GameListItem]))
)]
pub async fn list_games(
    State(state): State<SharedState>,
) -> Result<Json<Vec<GameListItem>>, AppError> {
    Ok(Json(admin_service::list_games(&state).await?))
}

/// Retrieve a game by its ID.
#[utoipa::path(
    get,
    path = "/admin/games/{id}",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream"),
    ("id" = String, Path, description = "Identifier of the game to retrieve")),
    responses((status = 200, description = "Game", body = GameSummary))
)]
pub async fn get_game_by_id(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<GameSummary>, AppError> {
    Ok(Json(admin_service::get_game_by_id(&state, id).await?))
}

/// Delete a persisted game by its identifier.
#[utoipa::path(
    delete,
    path = "/admin/games/{id}",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream"),
    ("id" = String, Path, description = "Identifier of the game to delete")),
    responses((status = 204, description = "Game deleted"))
)]
pub async fn delete_game(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    admin_service::delete_game(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Retrieve playlists eligible for generating new games.
#[utoipa::path(
    get,
    path = "/admin/playlists",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    responses((status = 200, description = "List available playlists", body = [PlaylistListItem]))
)]
pub async fn list_playlists(
    State(state): State<SharedState>,
) -> Result<Json<Vec<PlaylistListItem>>, AppError> {
    Ok(Json(admin_service::list_playlists(&state).await?))
}

/// Create a reusable playlist definition for later use in games.
#[utoipa::path(
    post,
    path = "/admin/playlists",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    request_body = PlaylistInput,
    responses((status = 200, description = "Playlist created", body = PlaylistSummary))
)]
pub async fn create_playlist(
    State(state): State<SharedState>,
    Json(payload): Json<PlaylistInput>,
) -> Result<Json<PlaylistSummary>, AppError> {
    Ok(Json(admin_service::create_playlist(&state, payload).await?))
}

/// Load and activate a stored game for continued play.
#[utoipa::path(
    post,
    path = "/admin/games/{id}/load",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream"),
    ("id" = String, Path, description = "Identifier of the game to load")),
    responses((status = 200, description = "Game loaded", body = GameSummary))
)]
pub async fn load_game(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<GameSummary>, AppError> {
    Ok(Json(admin_service::load_game(&state, id).await?))
}

/// Create a bespoke game definition under admin control.
#[utoipa::path(
    post,
    path = "/admin/games/with-playlist",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    request_body = CreateGameWithPlaylistRequest,
    responses((status = 200, description = "Game created", body = GameSummary))
)]
pub async fn create_game_with_playlist(
    State(state): State<SharedState>,
    Json(payload): Json<CreateGameWithPlaylistRequest>,
) -> Result<Json<GameSummary>, AppError> {
    Ok(Json(admin_service::create_game(&state, payload).await?))
}

/// Generate a game using an existing playlist as the source material.
#[utoipa::path(
    post,
    path = "/admin/games",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    request_body = CreateGameRequest,
    responses((status = 200, description = "Game created from playlist", body = GameSummary))
)]
pub async fn create_game(
    State(state): State<SharedState>,
    Json(payload): Json<CreateGameRequest>,
) -> Result<Json<GameSummary>, AppError> {
    let game = admin_service::create_game_from_playlist(&state, payload).await?;
    Ok(Json(game))
}

/// Begin a game session and publish the first song to admins.
#[utoipa::path(
    post,
    path = "/admin/game/start",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    responses((status = 200, description = "Game started", body = StartGameResponse))
)]
pub async fn start_game(
    State(state): State<SharedState>,
) -> Result<Json<StartGameResponse>, AppError> {
    Ok(Json(admin_service::start_game(&state).await?))
}

/// Pause the current game flow, freezing timers and buzzers.
#[utoipa::path(
    post,
    path = "/admin/game/pause",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    responses((status = 200, description = "Game paused", body = ActionResponse))
)]
pub async fn pause_game(
    State(state): State<SharedState>,
) -> Result<Json<ActionResponse>, AppError> {
    Ok(Json(admin_service::pause_game(&state).await?))
}

/// Resume a previously paused game.
#[utoipa::path(
    post,
    path = "/admin/game/resume",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    responses((status = 200, description = "Game resumed", body = ActionResponse))
)]
pub async fn resume_game(
    State(state): State<SharedState>,
) -> Result<Json<ActionResponse>, AppError> {
    Ok(Json(admin_service::resume_game(&state).await?))
}

/// Explicitly reveal the current song's answer to participants.
#[utoipa::path(
    post,
    path = "/admin/game/reveal",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    responses((status = 200, description = "Song revealed", body = ActionResponse))
)]
pub async fn reveal_song(
    State(state): State<SharedState>,
) -> Result<Json<ActionResponse>, AppError> {
    Ok(Json(admin_service::reveal(&state).await?))
}

/// Advance to the next song in the running game.
#[utoipa::path(
    post,
    path = "/admin/game/next",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    responses((status = 200, description = "Advanced to next song", body = NextSongResponse))
)]
pub async fn next_song(
    State(state): State<SharedState>,
) -> Result<Json<NextSongResponse>, AppError> {
    Ok(Json(admin_service::next_song(&state).await?))
}

/// Stop the game early and return final team standings.
#[utoipa::path(
    post,
    path = "/admin/game/stop",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    responses((status = 200, description = "Game stopped", body = StopGameResponse))
)]
pub async fn stop_game(
    State(state): State<SharedState>,
) -> Result<Json<StopGameResponse>, AppError> {
    Ok(Json(admin_service::stop_game(&state).await?))
}

/// Mark the game as finished and perform cleanup.
#[utoipa::path(
    post,
    path = "/admin/game/end",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    responses((status = 200, description = "Game ended", body = ActionResponse))
)]
pub async fn end_game(State(state): State<SharedState>) -> Result<Json<ActionResponse>, AppError> {
    Ok(Json(admin_service::end_game(&state).await?))
}

/// Flag a point or bonus field as discovered for the current song.
#[utoipa::path(
    post,
    path = "/admin/game/fields/found",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    request_body = MarkFieldRequest,
    responses((status = 200, description = "Updated discovered fields", body = FieldsFoundResponse))
)]
pub async fn mark_field_found(
    State(state): State<SharedState>,
    Json(payload): Json<MarkFieldRequest>,
) -> Result<Json<FieldsFoundResponse>, AppError> {
    let found_fields = admin_service::mark_field_found(&state, payload).await?;
    Ok(Json(found_fields))
}

/// Validate or reject the currently submitted answer.
#[utoipa::path(
    post,
    path = "/admin/game/answer",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    request_body = AnswerValidationRequest,
    responses((status = 200, description = "Answer validation applied", body = ActionResponse))
)]
pub async fn validate_answer(
    State(state): State<SharedState>,
    Json(payload): Json<AnswerValidationRequest>,
) -> Result<Json<ActionResponse>, AppError> {
    Ok(Json(admin_service::validate_answer(&state, payload).await?))
}

/// Adjust the score for a specific buzzer entry.
#[utoipa::path(
    post,
    path = "/admin/game/score",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    request_body = ScoreAdjustmentRequest,
    responses((status = 200, description = "Score adjusted", body = ScoreUpdateResponse))
)]
pub async fn adjust_score(
    State(state): State<SharedState>,
    Json(payload): Json<ScoreAdjustmentRequest>,
) -> Result<Json<ScoreUpdateResponse>, AppError> {
    Ok(Json(admin_service::adjust_score(&state, payload).await?))
}

#[utoipa::path(
    post,
    path = "/admin/teams",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    request_body = CreateTeamRequest,
    responses((status = 200, description = "Team created", body = TeamSummary))
)]
pub async fn create_team(
    State(state): State<SharedState>,
    Json(payload): Json<CreateTeamRequest>,
) -> Result<Json<TeamSummary>, AppError> {
    let summary = admin_service::create_team(&state, payload).await?;
    Ok(Json(summary))
}

#[utoipa::path(
    put,
    path = "/admin/teams/{id}",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream"),
    ("id" = Uuid, Path, description = "Identifier of the team to update")),
    request_body = UpdateTeamRequest,
    responses((status = 200, description = "Team updated", body = TeamSummary))
)]
pub async fn update_team(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Json(payload): Json<UpdateTeamRequest>,
) -> Result<Json<TeamSummary>, AppError> {
    let summary = admin_service::update_team(&state, id, payload).await?;
    Ok(Json(summary))
}

#[utoipa::path(
    delete,
    path = "/admin/teams/{id}",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream"),
    ("id" = Uuid, Path, description = "Identifier of the team to delete")),
    responses((status = 204, description = "Team deleted"))
)]
pub async fn delete_team(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    admin_service::delete_team(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/admin/teams/pairing",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    request_body = StartPairingRequest,
    responses((status = 202, description = "Pairing started"))
)]
pub async fn start_pairing(
    State(state): State<SharedState>,
    Json(payload): Json<StartPairingRequest>,
) -> Result<StatusCode, AppError> {
    admin_service::start_pairing(&state, payload).await?;
    Ok(StatusCode::ACCEPTED)
}

#[utoipa::path(
    post,
    path = "/admin/teams/pairing/abort",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    responses((status = 200, description = "Pairing aborted and roster restored", body = [TeamSummary]))
)]
pub async fn abort_pairing(
    State(state): State<SharedState>,
) -> Result<Json<Vec<TeamSummary>>, AppError> {
    let roster = admin_service::abort_pairing(&state).await?;
    Ok(Json(roster))
}

async fn require_admin_token(
    State(state): State<SharedState>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, AppError> {
    let provided = req
        .headers()
        .get(ADMIN_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_owned())
        .ok_or_else(|| {
            AppError::Unauthorized("missing admin token header `X-Admin-Token`".into())
        })?;

    let expected = {
        let guard = state.admin_token().lock().await;
        guard.clone()
    };

    match expected {
        Some(token) if token == provided => Ok(next.run(req).await),
        Some(_) => Err(AppError::Unauthorized("invalid admin token".into())),
        None => Err(AppError::Unauthorized(
            "admin SSE stream not initialised yet".into(),
        )),
    }
}
