use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use uuid::Uuid;

use crate::{
    dto::{
        admin::{
            ActionResponse, AnswerValidationRequest, CreateGameFromPlaylistRequest,
            FieldsFoundResponse, GameListItem, MarkFieldRequest, NextSongResponse,
            PlaylistListItem, ScoreAdjustmentRequest, ScoreUpdateResponse, StartGameResponse,
            StopGameResponse,
        },
        game::{CreateGameRequest, GameSummary},
    },
    error::AppError,
    services::admin_service,
    state::SharedState,
};

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/admin/games", get(list_games).post(create_game))
        .route(
            "/admin/games/from-playlist",
            post(create_game_from_playlist),
        )
        .route("/admin/games/{id}/load", post(load_game))
        .route("/admin/playlists", get(list_playlists))
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
}

async fn list_games(State(state): State<SharedState>) -> Result<Json<Vec<GameListItem>>, AppError> {
    Ok(Json(admin_service::list_games(&state).await?))
}

async fn list_playlists(
    State(state): State<SharedState>,
) -> Result<Json<Vec<PlaylistListItem>>, AppError> {
    Ok(Json(admin_service::list_playlists(&state).await?))
}

async fn load_game(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<GameSummary>, AppError> {
    Ok(Json(admin_service::load_game(&state, id).await?))
}

async fn create_game(
    State(state): State<SharedState>,
    Json(payload): Json<CreateGameRequest>,
) -> Result<Json<GameSummary>, AppError> {
    Ok(Json(admin_service::create_game(&state, payload).await?))
}

async fn create_game_from_playlist(
    State(state): State<SharedState>,
    Json(payload): Json<CreateGameFromPlaylistRequest>,
) -> Result<Json<GameSummary>, AppError> {
    let game = admin_service::create_game_from_playlist(&state, payload).await?;
    Ok(Json(game))
}

async fn start_game(State(state): State<SharedState>) -> Result<Json<StartGameResponse>, AppError> {
    Ok(Json(admin_service::start_game(&state).await?))
}

async fn pause_game(State(state): State<SharedState>) -> Result<Json<ActionResponse>, AppError> {
    Ok(Json(admin_service::pause_game(&state).await?))
}

async fn resume_game(State(state): State<SharedState>) -> Result<Json<ActionResponse>, AppError> {
    Ok(Json(admin_service::resume_game(&state).await?))
}

async fn reveal_song(State(state): State<SharedState>) -> Result<Json<ActionResponse>, AppError> {
    Ok(Json(admin_service::reveal(&state).await?))
}

async fn next_song(State(state): State<SharedState>) -> Result<Json<NextSongResponse>, AppError> {
    Ok(Json(admin_service::next_song(&state).await?))
}

async fn stop_game(State(state): State<SharedState>) -> Result<Json<StopGameResponse>, AppError> {
    Ok(Json(admin_service::stop_game(&state).await?))
}

async fn end_game(State(state): State<SharedState>) -> Result<Json<ActionResponse>, AppError> {
    Ok(Json(admin_service::end_game(&state).await?))
}

async fn mark_field_found(
    State(state): State<SharedState>,
    Json(payload): Json<MarkFieldRequest>,
) -> Result<Json<FieldsFoundResponse>, AppError> {
    let found_fields = admin_service::mark_field_found(&state, payload).await?;
    Ok(Json(found_fields))
}

async fn validate_answer(
    State(state): State<SharedState>,
    Json(payload): Json<AnswerValidationRequest>,
) -> Result<Json<ActionResponse>, AppError> {
    Ok(Json(admin_service::validate_answer(&state, payload).await?))
}

async fn adjust_score(
    State(state): State<SharedState>,
    Json(payload): Json<ScoreAdjustmentRequest>,
) -> Result<Json<ScoreUpdateResponse>, AppError> {
    Ok(Json(admin_service::adjust_score(&state, payload).await?))
}
