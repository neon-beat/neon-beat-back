//! Neon Beat Back binary entrypoint wiring REST, WebSocket, SSE, and MongoDB layers.

use std::{env, net::SocketAddr, time::Duration};

use anyhow::Context;
use axum::Router;
use tokio::net::TcpListener;
use tokio::time::sleep;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod dao;
mod dto;
mod error;
mod routes;
mod services;
mod state;

use dao::mongodb::{connect, ensure_indexes};
use state::{AppState, SharedState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let mongo_uri = env::var("MONGO_URI").unwrap_or_else(|_| "mongodb://localhost:27017".into());
    let mongo_db = env::var("MONGO_DB").ok();

    let app_state = AppState::new();

    tokio::spawn(run_mongo_supervisor(app_state.clone(), mongo_uri, mongo_db));
    // Build the HTTP router once the shared state is ready.
    let app = build_router(app_state);

    let port = env::var("PORT")
        .or_else(|_| env::var("SERVER_PORT"))
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!(%addr, "starting server");

    let listener = TcpListener::bind(addr).await.context("binding server")?;
    let service = app.into_make_service();
    axum::serve(listener, service)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serving axum")?;

    Ok(())
}

/// Supervises the MongoDB connection by retrying in the background and toggling
/// degraded mode when connectivity changes.
async fn run_mongo_supervisor(state: SharedState, uri: String, db_name: Option<String>) {
    let initial_delay_ms = 1000;
    let mut delay = Duration::from_millis(initial_delay_ms);
    let max_delay = Duration::from_secs(10);

    loop {
        if let Some(manager) = state.mongo().await {
            match manager.ping().await {
                Ok(_) => {
                    // Healthy connection: reset the retry backoff and avoid
                    // hammering the database with pings.
                    delay = Duration::from_millis(initial_delay_ms);
                    sleep(Duration::from_secs(5)).await;
                }
                Err(err) => {
                    // Existing connection failed: drop it, flip to degraded
                    // mode, and retry with exponential backoff.
                    warn!(error = %err, "MongoDB ping failed; entering degraded mode");
                    state.clear_mongo().await;
                    sleep(delay).await;
                    delay = (delay * 2).min(max_delay);
                }
            }
            continue;
        }

        match connect(&uri, db_name.as_deref()).await {
            Ok(manager) => match ensure_indexes(&manager.database().await).await {
                Ok(()) => {
                    // Fresh connection and indexes ready: install it and leave
                    // degraded mode.
                    info!("connected to MongoDB; leaving degraded mode");
                    state.install_mongo(manager).await;
                    delay = Duration::from_millis(initial_delay_ms);
                }
                Err(err) => {
                    // Connection succeeded but index creation failed: retry
                    // after backing off.
                    error!(%err, "failed to ensure MongoDB indexes; retrying");
                    sleep(delay).await;
                    delay = (delay * 2).min(max_delay);
                }
            },
            Err(err) => {
                // Could not reach MongoDB at all: wait and retry with
                // exponential backoff.
                warn!(error = %err, "MongoDB connection attempt failed");
                sleep(delay).await;
                delay = (delay * 2).min(max_delay);
            }
        }
    }
}

/// Build the top-level router and attach cross-cutting middleware layers.
fn build_router(state: state::SharedState) -> Router<()> {
    routes::router(state)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

/// Configure tracing subscribers so logs include spans by default.
fn init_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "info,tower_http=debug".into());
    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

/// Wait for Ctrl+C or SIGTERM and shut the server down gracefully.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = term.recv() => {},
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
