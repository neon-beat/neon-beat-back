use mongodb::error::Error as MongoError;
use thiserror::Error;
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, MongoDaoError>;

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
    #[error("failed to save game `{id}`")]
    SaveGame {
        id: Uuid,
        #[source]
        source: MongoError,
    },
    #[error("failed to save playlist `{id}`")]
    SavePlaylist {
        id: Uuid,
        #[source]
        source: MongoError,
    },
    #[error("failed to load game `{id}`")]
    LoadGame {
        id: Uuid,
        #[source]
        source: MongoError,
    },
    #[error("failed to load playlist `{id}`")]
    LoadPlaylist {
        id: Uuid,
        #[source]
        source: MongoError,
    },
    #[error("failed to list games")]
    ListGames {
        #[source]
        source: MongoError,
    },
    #[error("failed to list playlists")]
    ListPlaylists {
        #[source]
        source: MongoError,
    },
}
