mod config;
mod connection;
mod error;
mod models;
mod store;

pub use config::MongoConfig;
pub use error::MongoDaoError;
pub use store::MongoGameStore;

use crate::dao::storage::StorageError;

impl From<MongoDaoError> for StorageError {
    fn from(err: MongoDaoError) -> Self {
        StorageError::unavailable(err.to_string(), err)
    }
}
