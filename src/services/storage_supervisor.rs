use std::{future::Future, sync::Arc, time::Duration};

use tokio::time::sleep;
use tracing::{info, warn};

use crate::{
    dao::{game::GameStore, storage::StorageError},
    state::SharedState,
};

const INITIAL_DELAY: Duration = Duration::from_millis(1_000);
const MAX_DELAY: Duration = Duration::from_secs(10);
const HEALTH_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Reconnect to the storage backend and keep the shared state in degraded mode when it is unavailable.
pub async fn run<F, Fut>(
    state: SharedState,
    mut connect: F,
) where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = Result<Arc<dyn GameStore>, StorageError>> + Send,
{
    let mut delay = INITIAL_DELAY;
    let mut current: Option<Arc<dyn GameStore>> = None;

    loop {
        if let Some(store) = current.clone() {
            match store.health_check().await {
                Ok(()) => {
                    delay = INITIAL_DELAY;
                    sleep(HEALTH_POLL_INTERVAL).await;
                }
                Err(err) => {
                    warn!(error = %err, "storage health check failed; entering degraded mode");
                    state.clear_game_store().await;
                    current = None;
                    sleep(delay).await;
                    delay = (delay * 2).min(MAX_DELAY);
                }
            }
            continue;
        }

        match connect().await {
            Ok(store) => {
                state.install_game_store(store.clone()).await;
                info!("storage connection established; leaving degraded mode");
                current = Some(store);
                delay = INITIAL_DELAY;
            }
            Err(err) => {
                warn!(error = %err, "storage connection attempt failed");
                sleep(delay).await;
                delay = (delay * 2).min(MAX_DELAY);
            }
        }
    }
}
