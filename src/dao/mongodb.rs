use mongodb::{Client, Database, bson::doc, error::Error as MongoError, options::ClientOptions};
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tokio::{
    sync::RwLock,
    time::{MissedTickBehavior, interval, sleep},
};
use tracing::{error, info, warn};

const DEFAULT_DB: &str = "neon_beat";
const MAX_CONNECT_ATTEMPTS: u32 = 10;
const BASE_RETRY_DELAY_MS: u64 = 250;
const HEALTH_CHECK_INTERVAL_SECS: u64 = 30;

#[derive(Clone)]
pub struct MongoManager {
    inner: Arc<MongoManagerInner>,
}

struct MongoManagerInner {
    state: RwLock<MongoState>,
    options: ClientOptions,
    database_name: String,
    uri: String,
}

struct MongoState {
    client: Client,
    database: Database,
}

type Result<T> = std::result::Result<T, MongoDaoError>;

#[derive(Debug, Error)]
pub enum MongoDaoError {
    #[error("failed to parse MongoDB connection URI `{uri}`")]
    InvalidUri {
        uri: String,
        #[source]
        source: MongoError,
    },
    #[error("failed to build MongoDB client from options")]
    ClientConstruction {
        #[source]
        source: MongoError,
    },
    #[error("MongoDB ping failed during initial connection after {attempts} attempt(s)")]
    InitialPing {
        attempts: u32,
        #[source]
        source: MongoError,
    },
    #[error("MongoDB ping health check failed")]
    HealthPing {
        #[source]
        source: MongoError,
    },
    #[error("failed to ensure index `{index}` on collection `{collection}`")]
    EnsureIndex {
        collection: &'static str,
        index: &'static str,
        #[source]
        source: MongoError,
    },
}

/// Connect to MongoDB and start a watcher that keeps the connection healthy.
pub async fn connect(uri: &str, db_name: Option<&str>) -> Result<MongoManager> {
    let database_name = db_name.unwrap_or(DEFAULT_DB).to_owned();
    let options = ClientOptions::parse(uri)
        .await
        .map_err(|source| MongoDaoError::InvalidUri {
            uri: uri.to_owned(),
            source,
        })?;

    let (client, database) = establish_connection(&options, &database_name).await?;

    let state = MongoState { client, database };
    let inner = Arc::new(MongoManagerInner {
        state: RwLock::new(state),
        options,
        database_name,
        uri: uri.to_owned(),
    });

    MongoManagerInner::spawn_health_task(&inner);

    Ok(MongoManager { inner })
}

/// Ensure the indexes required by the application are present.
pub async fn ensure_indexes(database: &Database) -> Result<()> {
    let collection = database.collection::<mongodb::bson::Document>("game_state");
    let model = mongodb::IndexModel::builder()
        .keys(mongodb::bson::doc! {"quiz_name": 1})
        .build();
    collection
        .create_index(model)
        .await
        .map_err(|source| MongoDaoError::EnsureIndex {
            collection: "game_state",
            index: "quiz_name",
            source,
        })?;
    Ok(())
}

impl MongoManager {
    /// Clone the current database handle.
    pub async fn database(&self) -> Database {
        let guard = self.inner.state.read().await;
        guard.database.clone()
    }

    /// Issue a ping against the current MongoDB connection.
    pub async fn ping(&self) -> Result<()> {
        self.inner.ping().await
    }
}

impl MongoManagerInner {
    fn spawn_health_task(inner: &Arc<Self>) {
        let weak = Arc::downgrade(inner);
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(HEALTH_CHECK_INTERVAL_SECS));
            interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

            loop {
                interval.tick().await;

                let Some(inner) = weak.upgrade() else {
                    break;
                };

                if let Err(err) = inner.ping().await {
                    warn!(error = %err, "MongoDB health ping failed; attempting reconnect");
                    inner.reconnect().await;
                }
            }
        });
    }

    async fn ping(&self) -> Result<()> {
        let database = {
            let guard = self.state.read().await;
            guard.database.clone()
        };

        database
            .run_command(doc! { "ping": 1 })
            .await
            .map_err(|source| MongoDaoError::HealthPing { source })?;

        Ok(())
    }

    async fn reconnect(&self) {
        let mut attempt: u32 = 0;

        loop {
            attempt += 1;

            match establish_connection(&self.options, &self.database_name).await {
                Ok((client, database)) => {
                    {
                        let mut guard = self.state.write().await;
                        guard.client = client;
                        guard.database = database;
                    }
                    info!(attempt, "reconnected to MongoDB");
                    break;
                }
                Err(err) => {
                    error!(
                        attempt,
                        error = %err,
                        uri = %self.uri,
                        "MongoDB reconnect attempt failed"
                    );

                    let backoff_multiplier = 1u64 << (attempt.saturating_sub(1).min(4));
                    let wait = Duration::from_millis(BASE_RETRY_DELAY_MS * backoff_multiplier)
                        .min(Duration::from_secs(5));

                    sleep(wait).await;
                }
            }
        }
    }
}

async fn establish_connection(
    options: &ClientOptions,
    database_name: &str,
) -> Result<(Client, Database)> {
    let options = options.clone();
    let client = Client::with_options(options)
        .map_err(|source| MongoDaoError::ClientConstruction { source })?;
    let database = client.database(database_name);

    let mut attempt: u32 = 0;
    loop {
        attempt += 1;

        match database.run_command(doc! { "ping": 1 }).await {
            Ok(_) => {
                if attempt > 1 {
                    info!(attempt, "connected to MongoDB after retry");
                }
                return Ok((client.clone(), database.clone()));
            }
            Err(err) if attempt < MAX_CONNECT_ATTEMPTS => {
                let backoff_multiplier = 1u64 << (attempt.saturating_sub(1).min(4));
                let wait = Duration::from_millis(BASE_RETRY_DELAY_MS * backoff_multiplier)
                    .min(Duration::from_secs(5));
                warn!(
                    attempt,
                    wait_ms = wait.as_millis(),
                    error = %err,
                    "MongoDB ping failed during initial connection; retrying"
                );
                sleep(wait).await;
            }
            Err(err) => {
                return Err(MongoDaoError::InitialPing {
                    attempts: attempt,
                    source: err,
                });
            }
        }
    }
}

impl MongoManagerInner {}
