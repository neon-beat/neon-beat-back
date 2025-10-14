use std::{future::Future, sync::Arc, time::Duration};

use tokio::time::sleep;
use tracing::{info, warn};

use crate::{
    dao::{game_store::GameStore, storage::StorageError},
    state::SharedState,
};

const INITIAL_DELAY: Duration = Duration::from_millis(1_000);
const MAX_DELAY: Duration = Duration::from_secs(10);
const HEALTH_POLL_INTERVAL: Duration = Duration::from_secs(5);
const MAX_RECONNECT_ATTEMPTS: u32 = 3;

/// Reconnect to the storage backend and keep the shared state in degraded mode when it is unavailable.
pub async fn run<F, Fut>(state: SharedState, mut connect: F)
where
    F: FnMut() -> Fut + Send + 'static,
    Fut: Future<Output = Result<Arc<dyn GameStore>, StorageError>> + Send,
{
    let mut delay = INITIAL_DELAY;

    loop {
        match connect().await {
            Ok(store) => {
                state.set_game_store(store.clone()).await;
                info!("storage connection established; leaving degraded mode");
                delay = INITIAL_DELAY;

                loop {
                    match store.health_check().await {
                        Ok(()) => {
                            if state.is_degraded().await {
                                info!("storage healthy again; leaving degraded mode");
                                state.update_degraded(false).await;
                            }
                            sleep(HEALTH_POLL_INTERVAL).await;
                        }
                        Err(_) => {
                            let mut attempt = 0;
                            let mut reconnect_delay = INITIAL_DELAY;
                            let mut reconnected = false;

                            while attempt < MAX_RECONNECT_ATTEMPTS {
                                match store.try_reconnect().await {
                                    Ok(()) => {
                                        info!(
                                            "storage reconnection succeeded after health check failure"
                                        );
                                        reconnected = true;
                                        break;
                                    }
                                    Err(reconnect_err) => {
                                        if attempt == 0 {
                                            warn!(
                                                attempt, error = %reconnect_err,
                                                "storage reconnect first attempt failed; entering in degraded mode"
                                            );
                                            state.update_degraded(true).await;
                                        } else {
                                            warn!(attempt, error = %reconnect_err, "storage reconnect attempt failed");
                                        };
                                        attempt += 1;
                                        sleep(reconnect_delay).await;
                                        reconnect_delay = (reconnect_delay * 2).min(MAX_DELAY);
                                    }
                                }
                            }

                            if reconnected {
                                state.update_degraded(false).await;
                                sleep(HEALTH_POLL_INTERVAL).await;
                                continue;
                            } else {
                                warn!(
                                    "exhausted storage reconnect attempts; staying in degraded mode"
                                );
                                break;
                            }
                        }
                    }
                }

                sleep(delay).await;
                delay = (delay * 2).min(MAX_DELAY);
            }
            Err(err) => {
                warn!(error = %err, "storage connection attempt failed");
                sleep(delay).await;
                delay = (delay * 2).min(MAX_DELAY);
            }
        }
    }
}
