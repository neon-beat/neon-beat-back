use std::{sync::Arc, time::Duration};

use mongodb::{
    Client, Database,
    bson::doc,
    options::{ClientOptions, IndexOptions},
};
use tokio::{
    sync::RwLock,
    time::{MissedTickBehavior, interval, sleep},
};
use tracing::{info, warn};

use super::error::{MongoDaoError, Result};

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
}

struct MongoState {
    client: Client,
    database: Database,
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
    });

    MongoManagerInner::spawn_health_task(&inner);

    Ok(MongoManager { inner })
}

/// Ensure the indexes required by the application are present.
pub async fn ensure_indexes(database: &Database) -> Result<()> {
    let collection = database.collection::<mongodb::bson::Document>("games");
    let model = mongodb::IndexModel::builder()
        .keys(doc! {"name": 1})
        .options(
            IndexOptions::builder()
                .name(Some("game_name_idx".to_string()))
                .build(),
        )
        .build();
    collection
        .create_index(model)
        .await
        .map_err(|source| MongoDaoError::EnsureIndex {
            collection: "games",
            index: "name",
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
        let mut attempts = 0;
        let mut delay = Duration::from_millis(BASE_RETRY_DELAY_MS);

        loop {
            attempts += 1;
            match establish_connection(&self.options, &self.database_name).await {
                Ok((client, database)) => {
                    let mut guard = self.state.write().await;
                    guard.client = client;
                    guard.database = database;
                    info!("MongoDB connection re-established");
                    break;
                }
                Err(err) => {
                    warn!(
                        error = %err,
                        attempts,
                        "failed to re-establish MongoDB connection; retrying"
                    );
                    sleep(delay).await;
                    delay = (delay * 2).min(Duration::from_secs(5));
                }
            }
        }
    }
}

async fn establish_connection(
    options: &ClientOptions,
    database_name: &str,
) -> Result<(Client, Database)> {
    let client = Client::with_options(options.clone())
        .map_err(|source| MongoDaoError::ClientConstruction { source })?;
    let database = client.database(database_name);

    let mut attempts = 0;
    let mut interval = Duration::from_millis(BASE_RETRY_DELAY_MS);

    loop {
        match database.run_command(doc! { "ping": 1 }).await {
            Ok(_) => break,
            Err(err) => {
                attempts += 1;
                if attempts >= MAX_CONNECT_ATTEMPTS {
                    return Err(MongoDaoError::InitialPing {
                        attempts,
                        source: err,
                    });
                }
                sleep(interval).await;
                interval = (interval * 2).min(Duration::from_secs(5));
            }
        }
    }

    Ok((client, database))
}
