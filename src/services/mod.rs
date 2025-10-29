/// Admin service for game management operations.
pub mod admin_service;
/// OpenAPI documentation generation.
pub mod documentation;
/// Core game logic and state management.
pub mod game_service;
/// Health check service.
pub mod health_service;
/// Team pairing logic and utilities.
pub mod pairing;
/// Public service for read-only game information.
pub mod public_service;
/// Server-Sent Events message generation.
pub mod sse_events;
/// Server-Sent Events broadcasting service.
pub mod sse_service;
/// Storage persistence coordinator with debouncing.
pub mod storage_supervisor;
/// WebSocket connection and message handling service.
pub mod websocket_service;
