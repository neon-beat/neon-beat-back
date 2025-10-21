mod config;
mod error;
mod models;
mod store;

pub use config::CouchConfig;
use error::CouchDaoError;
pub use store::CouchGameStore;

use crate::dao::storage::StorageError;

impl From<CouchDaoError> for StorageError {
    fn from(err: CouchDaoError) -> Self {
        StorageError::unavailable(err.to_string(), err)
    }
}
