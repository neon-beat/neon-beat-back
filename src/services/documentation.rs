use utoipa::OpenApi;

#[derive(OpenApi)]
/// Aggregated OpenAPI specification for Neon Beat Back.
#[openapi(
    paths(
        crate::routes::health::healthcheck,
        crate::routes::sse::public_stream,
        crate::routes::sse::admin_stream,
        crate::routes::websocket::ws_handler,
        crate::routes::game::create_game,
        crate::routes::game::load_game,
        crate::routes::admin::list_games,
        crate::routes::admin::list_playlists,
        crate::routes::admin::load_game,
        crate::routes::admin::create_game,
        crate::routes::admin::create_game_from_playlist,
        crate::routes::admin::start_game,
        crate::routes::admin::pause_game,
        crate::routes::admin::resume_game,
        crate::routes::admin::reveal_song,
        crate::routes::admin::next_song,
        crate::routes::admin::stop_game,
        crate::routes::admin::end_game,
        crate::routes::admin::mark_field_found,
        crate::routes::admin::validate_answer,
        crate::routes::admin::adjust_score,
    ),
    components(
        schemas(
            crate::dto::health::HealthResponse,
            crate::dto::ws::BuzzerInboundMessage,
            crate::dto::ws::BuzzerAck,
            crate::dto::game::CreateGameRequest,
            crate::dto::game::PlayerInput,
            crate::dto::game::PlaylistInput,
            crate::dto::game::SongInput,
            crate::dto::game::PlayerSummary,
            crate::dto::game::GameSummary,
            crate::dto::game::PlaylistSummary,
            crate::dto::game::SongSummary,
            crate::dto::game::PointFieldSummary,
            crate::dto::sse::SystemStatus,
            crate::dto::sse::Handshake,
            crate::dto::sse::TeamSummary,
            crate::dto::admin::GameListItem,
            crate::dto::admin::PlaylistListItem,
            crate::dto::admin::CreateGameFromPlaylistRequest,
            crate::dto::admin::FieldKind,
            crate::dto::admin::MarkFieldRequest,
            crate::dto::admin::FieldsFoundResponse,
            crate::dto::admin::AnswerValidationRequest,
            crate::dto::admin::ScoreAdjustmentRequest,
            crate::dto::admin::ActionResponse,
            crate::dto::admin::ScoreUpdateResponse,
            crate::dto::admin::StartGameResponse,
            crate::dto::admin::NextSongResponse,
            crate::dto::admin::StopGameResponse,
        )
    ),
    tags(
        (name = "health", description = "Health check endpoints"),
        (name = "sse", description = "Server-sent events streams"),
        (name = "buzzers", description = "WebSocket operations for buzzer devices"),
        (name = "game", description = "Game bootstrap operations"),
        (name = "admin", description = "Administrative controls for running games"),
    )
)]
pub struct ApiDoc;
