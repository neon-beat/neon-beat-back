use std::time::SystemTime;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

pub mod admin;
pub mod common;
pub mod game;
pub mod health;
pub mod phase;
pub mod public;
pub mod sse;
pub mod validation;
pub mod ws;

fn format_system_time(time: SystemTime) -> String {
    OffsetDateTime::from(time)
        .format(&Rfc3339)
        .unwrap_or_else(|_| "invalid-timestamp".into())
}
