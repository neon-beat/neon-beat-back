use std::time::SystemTime;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

/// Admin API data structures.
pub mod admin;
/// Common data structures shared across DTOs.
pub mod common;
/// Game session data structures.
pub mod game;
/// Health check data structures.
pub mod health;
/// Game phase data structures.
pub mod phase;
/// Public API data structures.
pub mod public;
/// Server-Sent Events data structures.
pub mod sse;
/// Request validation utilities.
pub mod validation;
/// WebSocket message data structures.
pub mod ws;

/// Formats a SystemTime as an RFC3339 timestamp string.
fn format_system_time(time: SystemTime) -> String {
    OffsetDateTime::from(time)
        .format(&Rfc3339)
        .unwrap_or_else(|_| "invalid-timestamp".into())
}
