//! Neon Beat Back binary entrypoint wiring REST, WebSocket, SSE, and MongoDB layers.

use std::{env, net::SocketAddr};

use anyhow::Context;
use axum::Router;
use tokio::net::TcpListener;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod dao;
mod dto;
mod error;
mod routes;
mod services;
mod state;

use dao::mongodb::{connect, ensure_indexes};
use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let mongo_uri = env::var("MONGO_URI").unwrap_or_else(|_| "mongodb://localhost:27017".into());
    let mongo_db = env::var("MONGO_DB").ok();

    let mongo_manager = connect(&mongo_uri, mongo_db.as_deref())
        .await
        .context("connecting to MongoDB")?;
    let indexes_database = mongo_manager.database().await;
    ensure_indexes(&indexes_database)
        .await
        .context("ensuring MongoDB indexes")?;

    let app_state = AppState::new(mongo_manager);
    // Build the HTTP router once the shared state is ready.
    let app = build_router(app_state.clone());

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
