use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use uuid::Uuid;

use crate::{
    dto::game::{CreateGameRequest, GameSummary},
    error::AppError,
    services::game_service,
    state::SharedState,
};

/// Routes handling game bootstrap operations (creation & loading).
pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/games", post(create_game))
        .route("/games/{id}/load", post(load_game))
}

/// Create a fresh game definition and persist it.
#[utoipa::path(
    post,
    path = "/games",
    tag = "game",
    request_body = CreateGameRequest,
    responses(
        (status = 200, description = "Game created", body = GameSummary)
    )
)]
pub async fn create_game(
    State(state): State<SharedState>,
    Json(payload): Json<CreateGameRequest>,
) -> Result<Json<GameSummary>, AppError> {
    let summary = game_service::create_game(&state, payload).await?;
    Ok(Json(summary))
}

/// Load an existing game from storage and prime the shared state.
#[utoipa::path(
    post,
    path = "/games/{id}/load",
    tag = "game",
    params(("id" = String, Path, description = "Identifier of the game to load")),
    responses(
        (status = 200, description = "Game loaded", body = GameSummary)
    )
)]
pub async fn load_game(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
) -> Result<Json<GameSummary>, AppError> {
    let summary = game_service::load_game(&state, id).await?;
    Ok(Json(summary))
}
