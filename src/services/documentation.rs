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
        )
    ),
    tags(
        (name = "health", description = "Health check endpoints"),
        (name = "sse", description = "Server-sent events streams"),
        (name = "buzzers", description = "WebSocket operations for buzzer devices"),
        (name = "game", description = "Game bootstrap operations"),
    )
)]
pub struct ApiDoc;
