mod config;
mod error;
mod models;
mod store;

#[allow(unused_imports)]
pub use config::CouchConfig;
#[allow(unused_imports)]
pub use error::{CouchDaoError, CouchResult};
#[allow(unused_imports)]
pub use store::CouchGameStore;

use crate::dao::storage::StorageError;

impl From<CouchDaoError> for StorageError {
    fn from(err: CouchDaoError) -> Self {
        StorageError::unavailable(err.to_string(), err)
    }
}
