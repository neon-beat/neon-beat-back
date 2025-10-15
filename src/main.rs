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

use dao::game_store::GameStore;
#[cfg(feature = "couch-store")]
use dao::game_store::couchdb::{CouchConfig, CouchGameStore};
#[cfg(feature = "mongo-store")]
use dao::game_store::mongodb::{MongoConfig, MongoGameStore};
use services::storage_supervisor;
use state::AppState;

#[cfg(not(any(feature = "mongo-store", feature = "couch-store")))]
compile_error!(
    "At least one storage backend feature (`mongo-store` or `couch-store`) must be enabled."
);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let app_state = AppState::new();

    let backend = select_store()?;

    match backend {
        #[cfg(feature = "mongo-store")]
        StoreKind::Mongo => {
            spawn_mongo_supervisor(app_state.clone()).await?;
        }
        #[cfg(feature = "couch-store")]
        StoreKind::Couch => {
            spawn_couch_supervisor(app_state.clone()).await?;
        }
    }

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

#[cfg(feature = "mongo-store")]
/// Launch the storage supervisor task responsible for maintaining the MongoDB connection.
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

#[cfg(feature = "couch-store")]
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

/// Enumerates the storage backends compiled into the current binary.
#[derive(Debug, Clone, Copy)]
enum StoreKind {
    #[cfg(feature = "mongo-store")]
    /// Storage backed by MongoDB.
    Mongo,
    #[cfg(feature = "couch-store")]
    /// Storage backed by CouchDB.
    Couch,
}

/// Resolve which storage backend should be booted for this process.
fn select_store() -> anyhow::Result<StoreKind> {
    match std::env::var("NEON_STORE") {
        Ok(value) => resolve_store(&value).map_err(|message| anyhow::anyhow!(message)),
        Err(std::env::VarError::NotPresent) => default_store(),
        Err(err) => Err(err.into()),
    }
}

#[cfg(feature = "mongo-store")]
/// Check whether the provided value selects the MongoDB backend.
fn is_mongo(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.eq_ignore_ascii_case("mongo") || trimmed.eq_ignore_ascii_case("mongodb")
}

#[cfg(feature = "couch-store")]
/// Check whether the provided value selects the CouchDB backend.
fn is_couch(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.eq_ignore_ascii_case("couch") || trimmed.eq_ignore_ascii_case("couchdb")
}

/// Determine the store to use when no explicit `NEON_STORE` is provided.
fn default_store() -> anyhow::Result<StoreKind> {
    #[cfg(all(feature = "mongo-store", feature = "couch-store"))]
    {
        anyhow::bail!(
            "NEON_STORE must be set to `mongo` or `couch` when both storage backends are compiled"
        )
    }
    #[cfg(all(feature = "mongo-store", not(feature = "couch-store")))]
    {
        Ok(StoreKind::Mongo)
    }
    #[cfg(all(feature = "couch-store", not(feature = "mongo-store")))]
    {
        Ok(StoreKind::Couch)
    }
}

/// Interpret a `NEON_STORE` value and map it to the compiled backend.
fn resolve_store(value: &str) -> Result<StoreKind, String> {
    #[cfg(all(feature = "mongo-store", feature = "couch-store"))]
    {
        if is_mongo(value) {
            Ok(StoreKind::Mongo)
        } else if is_couch(value) {
            Ok(StoreKind::Couch)
        } else {
            Err(format!(
                "Invalid NEON_STORE value `{value}` (expected `mongo` or `couch`)"
            ))
        }
    }
    #[cfg(all(feature = "mongo-store", not(feature = "couch-store")))]
    {
        if is_mongo(value) {
            Ok(StoreKind::Mongo)
        } else {
            Err(format!(
                "Invalid NEON_STORE value `{value}`; this binary was compiled with only the Mongo backend"
            ))
        }
    }
    #[cfg(all(feature = "couch-store", not(feature = "mongo-store")))]
    {
        if is_couch(value) {
            Ok(StoreKind::Couch)
        } else {
            Err(format!(
                "Invalid NEON_STORE value `{value}`; this binary was compiled with only the Couch backend"
            ))
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
