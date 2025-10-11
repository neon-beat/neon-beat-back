use std::error::Error;
use thiserror::Error;

/// Result alias for storage operations.
pub type StorageResult<T> = Result<T, StorageError>;

/// Error raised by storage backends regardless of the underlying database.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("storage unavailable: {message}")]
    Unavailable {
        message: String,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

impl StorageError {
    /// Construct an unavailable error from any backend failure.
    pub fn unavailable(message: String, source: impl Error + Send + Sync + 'static) -> Self {
        StorageError::Unavailable {
            message,
            source: Box::new(source),
        }
    }
}
