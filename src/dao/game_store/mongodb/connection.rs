use std::time::Duration;

use mongodb::{Client, Database, bson::doc, options::ClientOptions};
use tokio::time::sleep;

use super::error::{MongoDaoError, MongoResult};

struct RetryPolicy;

impl RetryPolicy {
    const MAX_ATTEMPTS: u32 = 10;
    const INITIAL_DELAY_MS: u64 = 250;

    fn initial_delay() -> Duration {
        Duration::from_millis(Self::INITIAL_DELAY_MS)
    }

    fn next_delay(current: Duration) -> Duration {
        (current * 2).min(Duration::from_secs(5))
    }
}

pub async fn establish_connection(
    options: &ClientOptions,
    database_name: &str,
) -> MongoResult<(Client, Database)> {
    let client = Client::with_options(options.clone())
        .map_err(|source| MongoDaoError::ClientConstruction { source })?;
    let database = client.database(database_name);

    let mut attempts = 0;
    let mut delay = RetryPolicy::initial_delay();

    loop {
        match database.run_command(doc! { "ping": 1 }).await {
            Ok(_) => break,
            Err(err) => {
                attempts += 1;
                if attempts >= RetryPolicy::MAX_ATTEMPTS {
                    return Err(MongoDaoError::InitialPing {
                        attempts,
                        source: err,
                    });
                }
                sleep(delay).await;
                delay = RetryPolicy::next_delay(delay);
            }
        }
    }

    Ok((client, database))
}
