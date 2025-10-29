//! Library crate for neon-beat-back, exposing modules for binaries and integration tests.

/// Configuration module for application settings.
mod config;
/// Data Access Object module for database operations.
pub mod dao;
/// Data Transfer Object module for API request/response structures.
mod dto;
/// Error handling module with custom error types.
mod error;
/// HTTP routes module for API endpoints.
pub mod routes;
/// Business logic services module.
pub mod services;
/// Application state management module.
pub mod state;
