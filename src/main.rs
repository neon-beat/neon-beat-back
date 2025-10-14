//! Neon Beat Back binary entrypoint wiring REST, WebSocket, SSE, and MongoDB layers.

use std::{env, net::SocketAddr, sync::Arc};

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

use dao::game_store::{
    GameStore,
    couchdb::{CouchConfig, CouchGameStore},
    mongodb::{MongoConfig, MongoGameStore},
};
use services::storage_supervisor;
use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let app_state = AppState::new();

    spawn_couch_supervisor(app_state.clone()).await?;

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

/// Launch the storage supervisor task responsible for maintaining the MongoDB connection.
#[allow(dead_code)]
async fn spawn_mongo_supervisor(state: Arc<AppState>) -> anyhow::Result<()> {
    let config = Arc::new(MongoConfig::from_env().await?);

    tokio::spawn(storage_supervisor::run(state, {
        move || {
            let cfg = config.clone();
            async move {
                let store = MongoGameStore::connect((*cfg).clone()).await?;
                Ok::<Arc<dyn GameStore>, _>(Arc::new(store))
            }
        }
    }));

    Ok(())
}

/// Launch the storage supervisor task responsible for maintaining the CouchDB connection.
async fn spawn_couch_supervisor(state: Arc<AppState>) -> anyhow::Result<()> {
    let config = Arc::new(CouchConfig::from_env()?);

    tokio::spawn(storage_supervisor::run(state, {
        move || {
            let cfg = config.clone();
            async move {
                let store = CouchGameStore::connect((*cfg).clone()).await?;
                Ok::<Arc<dyn GameStore>, _>(Arc::new(store))
            }
        }
    }));

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
