use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};

use crate::{
    dto::{
        admin::NoQuery,
        public::{CurrentSongResponse, GamePhaseResponse, PairingStatusResponse, TeamsResponse},
    },
    error::AppError,
    services::public_service,
    state::SharedState,
};

/// Public read-only endpoints that expose the current game state.
pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/public/teams", get(get_teams))
        .route("/public/song", get(get_current_song))
        .route("/public/phase", get(get_game_phase))
        .route("/public/pairing", get(get_pairing_status))
}

#[utoipa::path(
    get,
    path = "/public/teams",
    tag = "public",
    responses((status = 200, description = "Current teams", body = TeamsResponse))
)]
/// Return the teams currently participating in the game.
pub async fn get_teams(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
) -> Result<Json<TeamsResponse>, AppError> {
    let payload = public_service::get_teams(&state).await?;
    Ok(Json(payload))
}

#[utoipa::path(
    get,
    path = "/public/song",
    tag = "public",
    responses(
        (status = 200, description = "Current song", body = CurrentSongResponse),
        (status = 404, description = "No active song")
    )
)]
/// Return the song currently being played and progress made so far.
pub async fn get_current_song(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
) -> Result<Json<CurrentSongResponse>, AppError> {
    let payload = public_service::get_current_song(&state).await?;
    Ok(Json(payload))
}

#[utoipa::path(
    get,
    path = "/public/phase",
    tag = "public",
    responses((status = 200, description = "Current game phase", body = GamePhaseResponse))
)]
/// Return the high-level phase the game is currently in.
pub async fn get_game_phase(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
) -> Result<Json<GamePhaseResponse>, AppError> {
    let payload = public_service::get_game_phase(&state).await?;
    Ok(Json(payload))
}

#[utoipa::path(
    get,
    path = "/public/pairing",
    tag = "public",
    responses((status = 200, description = "Current pairing status", body = PairingStatusResponse))
)]
/// Return the current pairing workflow status for public clients.
pub async fn get_pairing_status(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
) -> Result<Json<PairingStatusResponse>, AppError> {
    let payload = public_service::get_pairing_status(&state).await?;
    Ok(Json(payload))
}
