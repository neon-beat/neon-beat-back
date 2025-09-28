use utoipa::OpenApi;

#[derive(OpenApi)]
/// Aggregated OpenAPI specification for Neon Beat Back.
#[openapi(
    paths(
        crate::routes::health::healthcheck,
        crate::routes::sse::public_stream,
        crate::routes::sse::admin_stream,
        crate::routes::websocket::ws_handler,
    ),
    components(
        schemas(
            crate::dto::health::HealthResponse,
            crate::dto::ws::BuzzerInboundMessage,
            crate::dto::ws::BuzzerAck,
            crate::dto::sse::AdminHandshake,
            crate::dao::models::GameState,
        )
    ),
    tags(
        (name = "health", description = "Health check endpoints"),
        (name = "sse", description = "Server-sent events streams"),
        (name = "buzzers", description = "WebSocket operations for buzzer devices"),
    )
)]
pub struct ApiDoc;
