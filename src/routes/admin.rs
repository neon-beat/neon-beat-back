#![allow(deprecated)]

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post, put},
};
use axum_valid::Valid;
use uuid::Uuid;

use crate::{
    dto::{
        admin::{
            ActionResponse, AnswerFoundRequest, AnswersFoundResponse, CreateGameQuery,
            CreateGameRequest, CreateTeamRequest, GameListItem, LoadGameQuery,
            NextQuestionResponse, NoQuery, QuestionHintRequest, QuestionHintsResponse,
            QuestionValidationRequest, QuestionsSequenceListItem, ScoreAdjustmentRequest,
            ScoreUpdateResponse, StartGameResponse, StartPairingRequest, StopGameResponse,
            UpdateTeamRequest,
        },
        game::{
            CreateGameWithQuestionsSequenceRequest, GameSummary, QuestionsSequenceInput,
            QuestionsSequenceSummary, TeamSummary,
        },
    },
    error::AppError,
    services::admin_service,
    state::SharedState,
};

use crate::dto::{
    admin::LegacyPlaylistListItem,
    game::{LegacyCreateGameWithPlaylistRequest, LegacyPlaylistInput},
};

const ADMIN_TOKEN_HEADER: &str = "x-admin-token";

/// Admin-only management endpoints for configuring and driving games.
pub fn router(state: SharedState) -> Router<SharedState> {
    Router::new()
        .route("/admin/games", get(list_games).post(create_game))
        .route(
            "/admin/games/with-questions-sequence",
            post(create_game_with_questions_sequence),
        )
        .route(
            "/admin/games/with-playlist",
            post(create_game_with_playlist),
        )
        .route("/admin/games/{id}", get(get_game_by_id).delete(delete_game))
        .route("/admin/games/{id}/load", post(load_game))
        .route(
            "/admin/questions-sequence",
            get(list_questions_sequences).post(create_questions_sequence),
        )
        .route(
            "/admin/playlists",
            get(list_playlists).post(create_deprecated_playlist),
        )
        .route("/admin/game/start", post(start_game))
        .route("/admin/game/pause", post(pause_game))
        .route("/admin/game/resume", post(resume_game))
        .route("/admin/game/reveal", post(reveal_question))
        .route("/admin/game/next", post(next_question))
        .route("/admin/game/stop", post(stop_game))
        .route("/admin/game/end", post(end_game))
        .route("/admin/game/question/answer-found", post(mark_answer_found))
        .route(
            "/admin/game/question/submit-validation",
            post(submit_question_validation),
        )
        .route("/admin/game/question/hint", post(reveal_hint))
        .route("/admin/teams/{id}/score", post(adjust_score))
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
    Query(_no_query): Query<NoQuery>,
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
    Query(_no_query): Query<NoQuery>,
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
    Query(_no_query): Query<NoQuery>,
) -> Result<StatusCode, AppError> {
    admin_service::delete_game(&state, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Retrieve questions sequences eligible for generating new games.
#[utoipa::path(
    get,
    path = "/admin/questions-sequence",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    responses((status = 200, description = "List available questions sequences", body = [QuestionsSequenceListItem]))
)]
pub async fn list_questions_sequences(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
) -> Result<Json<Vec<QuestionsSequenceListItem>>, AppError> {
    Ok(Json(admin_service::list_questions_sequences(&state).await?))
}

/// Deprecated alias for retrieving questions sequences as legacy playlists.
#[utoipa::path(
    get,
    path = "/admin/playlists",
    tag = "admin",
    summary = "Deprecated: list legacy playlists",
    description = "Deprecated since 0.9.0. Use GET /admin/questions-sequence instead.",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    responses((status = 200, description = "List available legacy playlists", body = [LegacyPlaylistListItem]))
)]
#[deprecated(since = "0.9.0", note = "Use GET /admin/questions-sequence instead.")]
pub async fn list_playlists(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
) -> Result<Json<Vec<LegacyPlaylistListItem>>, AppError> {
    Ok(Json(admin_service::list_legacy_playlists(&state).await?))
}

/// Create a reusable questions sequence definition for later use in games.
#[utoipa::path(
    post,
    path = "/admin/questions-sequence",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    request_body = QuestionsSequenceInput,
    responses((status = 200, description = "Questions sequence created", body = QuestionsSequenceSummary))
)]
pub async fn create_questions_sequence(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
    Valid(Json(payload)): Valid<Json<QuestionsSequenceInput>>,
) -> Result<Json<QuestionsSequenceSummary>, AppError> {
    Ok(Json(
        admin_service::create_questions_sequence(&state, payload).await?,
    ))
}

/// Deprecated alias for creating a sequence from a legacy playlist payload.
#[utoipa::path(
    post,
    path = "/admin/playlists",
    tag = "admin",
    summary = "Deprecated: create a questions sequence from a legacy playlist",
    description = "Deprecated since 0.9.0. Use POST /admin/questions-sequence instead.",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    request_body = LegacyPlaylistInput,
    responses((status = 200, description = "Questions sequence created", body = QuestionsSequenceSummary))
)]
#[deprecated(since = "0.9.0", note = "Use POST /admin/questions-sequence instead.")]
pub async fn create_deprecated_playlist(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
    Valid(Json(payload)): Valid<Json<LegacyPlaylistInput>>,
) -> Result<Json<QuestionsSequenceSummary>, AppError> {
    let payload = payload.into();
    Ok(Json(
        admin_service::create_questions_sequence(&state, payload).await?,
    ))
}

/// Load and activate a stored game for continued play.
#[utoipa::path(
    post,
    path = "/admin/games/{id}/load",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream"),
    ("id" = String, Path, description = "Identifier of the game to load"),
    ("shuffle" = Option<bool>, Query, description = "Shuffle questions (default false) ; only applies when loading a game that has not yet started or whose sequence is completely played")),
    responses((status = 200, description = "Game loaded", body = GameSummary))
)]
pub async fn load_game(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Query(options): Query<LoadGameQuery>,
) -> Result<Json<GameSummary>, AppError> {
    Ok(Json(
        admin_service::load_game(&state, id, options.shuffle).await?,
    ))
}

/// Create a bespoke game definition under admin control.
#[utoipa::path(
    post,
    path = "/admin/games/with-questions-sequence",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream"),
    ("shuffle" = Option<bool>, Query, description = "Shuffle questions (default false)")),
    request_body = CreateGameWithQuestionsSequenceRequest,
    responses((status = 200, description = "Game created", body = GameSummary))
)]
pub async fn create_game_with_questions_sequence(
    State(state): State<SharedState>,
    Query(options): Query<CreateGameQuery>,
    Valid(Json(payload)): Valid<Json<CreateGameWithQuestionsSequenceRequest>>,
) -> Result<Json<GameSummary>, AppError> {
    Ok(Json(
        admin_service::create_game(&state, payload, options.shuffle).await?,
    ))
}

/// Deprecated alias for creating a game with an inline legacy playlist.
#[utoipa::path(
    post,
    path = "/admin/games/with-playlist",
    tag = "admin",
    summary = "Deprecated: create a game with a legacy playlist",
    description = "Deprecated since 0.9.0. Use POST /admin/games/with-questions-sequence instead.",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream"),
    ("shuffle" = Option<bool>, Query, description = "Shuffle questions (default false)")),
    request_body = LegacyCreateGameWithPlaylistRequest,
    responses((status = 200, description = "Game created", body = GameSummary))
)]
#[deprecated(
    since = "0.9.0",
    note = "Use POST /admin/games/with-questions-sequence instead."
)]
pub async fn create_game_with_playlist(
    State(state): State<SharedState>,
    Query(options): Query<CreateGameQuery>,
    Valid(Json(payload)): Valid<Json<LegacyCreateGameWithPlaylistRequest>>,
) -> Result<Json<GameSummary>, AppError> {
    let payload = payload.into();
    Ok(Json(
        admin_service::create_game(&state, payload, options.shuffle).await?,
    ))
}

/// Generate a game using an existing questions sequence as the source material.
#[utoipa::path(
    post,
    path = "/admin/games",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream"),
    ("shuffle" = Option<bool>, Query, description = "Shuffle questions (default false)")),
    request_body = CreateGameRequest,
    responses((status = 200, description = "Game created from questions sequence", body = GameSummary))
)]
pub async fn create_game(
    State(state): State<SharedState>,
    Query(options): Query<CreateGameQuery>,
    Valid(Json(payload)): Valid<Json<CreateGameRequest>>,
) -> Result<Json<GameSummary>, AppError> {
    let game = admin_service::create_game_from_questions_sequence(&state, payload, options.shuffle)
        .await?;
    Ok(Json(game))
}

/// Begin a game session and publish the first question to admins.
#[utoipa::path(
    post,
    path = "/admin/game/start",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    responses((status = 200, description = "Game started", body = StartGameResponse))
)]
pub async fn start_game(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
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
    Query(_no_query): Query<NoQuery>,
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
    Query(_no_query): Query<NoQuery>,
) -> Result<Json<ActionResponse>, AppError> {
    Ok(Json(admin_service::resume_game(&state).await?))
}

/// Explicitly reveal the current question's answer to participants.
#[utoipa::path(
    post,
    path = "/admin/game/reveal",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    responses((status = 200, description = "Question revealed", body = ActionResponse))
)]
pub async fn reveal_question(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
) -> Result<Json<ActionResponse>, AppError> {
    Ok(Json(admin_service::reveal(&state).await?))
}

/// Advance to the next question in the running game.
#[utoipa::path(
    post,
    path = "/admin/game/next",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    responses((status = 200, description = "Advanced to next question", body = NextQuestionResponse))
)]
pub async fn next_question(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
) -> Result<Json<NextQuestionResponse>, AppError> {
    Ok(Json(admin_service::next_question(&state).await?))
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
    Query(_no_query): Query<NoQuery>,
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
pub async fn end_game(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
) -> Result<Json<ActionResponse>, AppError> {
    Ok(Json(admin_service::end_game(&state).await?))
}

/// Flag an answer as discovered for the current question.
#[utoipa::path(
    post,
    path = "/admin/game/question/answer-found",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    request_body = AnswerFoundRequest,
    responses((status = 200, description = "Updated discovered answers", body = AnswersFoundResponse))
)]
pub async fn mark_answer_found(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
    Json(payload): Json<AnswerFoundRequest>,
) -> Result<Json<AnswersFoundResponse>, AppError> {
    let found_answers = admin_service::mark_answer_found(&state, payload).await?;
    Ok(Json(found_answers))
}

/// Reveal a hint for the current question.
#[utoipa::path(
    post,
    path = "/admin/game/question/hint",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    request_body = QuestionHintRequest,
    responses((status = 200, description = "Updated revealed hints", body = QuestionHintsResponse))
)]
pub async fn reveal_hint(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
    Json(payload): Json<QuestionHintRequest>,
) -> Result<Json<QuestionHintsResponse>, AppError> {
    let hints = admin_service::reveal_hint(&state, payload).await?;
    Ok(Json(hints))
}

/// Submit an admin validation decision for the currently submitted answer.
#[utoipa::path(
    post,
    path = "/admin/game/question/submit-validation",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    request_body = QuestionValidationRequest,
    responses((status = 200, description = "Answer validation applied", body = ActionResponse))
)]
pub async fn submit_question_validation(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
    Json(payload): Json<QuestionValidationRequest>,
) -> Result<Json<ActionResponse>, AppError> {
    Ok(Json(
        admin_service::submit_question_validation(&state, payload).await?,
    ))
}

/// Adjust the score for a specific team by team ID.
#[utoipa::path(
    post,
    path = "/admin/teams/{id}/score",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream"),
            ("id" = String, Path, description = "Identifier of the team to adjust")),
    request_body = ScoreAdjustmentRequest,
    responses((status = 200, description = "Score adjusted", body = ScoreUpdateResponse))
)]
pub async fn adjust_score(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Query(_no_query): Query<NoQuery>,
    Json(payload): Json<ScoreAdjustmentRequest>,
) -> Result<Json<ScoreUpdateResponse>, AppError> {
    Ok(Json(
        admin_service::adjust_score(&state, id, payload).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/admin/teams",
    tag = "admin",
    params(("X-Admin-Token" = String, Header, description = "Admin token issued by the /sse/admin stream")),
    request_body = CreateTeamRequest,
    responses((status = 200, description = "Team created", body = TeamSummary))
)]
/// Create a new team in the active game during prep phase.
pub async fn create_team(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
    Valid(Json(payload)): Valid<Json<CreateTeamRequest>>,
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
/// Update an existing team's information (name, buzzer, score, or color).
pub async fn update_team(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Query(_no_query): Query<NoQuery>,
    Valid(Json(payload)): Valid<Json<UpdateTeamRequest>>,
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
/// Delete a team from the active game during prep phase.
pub async fn delete_team(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Query(_no_query): Query<NoQuery>,
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
/// Start the buzzer pairing workflow for teams during prep phase.
pub async fn start_pairing(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
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
/// Abort the buzzer pairing workflow and restore teams to their pre-pairing state.
pub async fn abort_pairing(
    State(state): State<SharedState>,
    Query(_no_query): Query<NoQuery>,
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
