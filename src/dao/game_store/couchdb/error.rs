//! Error types shared by the CouchDB storage implementation.

use reqwest::StatusCode;
use thiserror::Error;

/// Convenient result alias returning [`CouchDaoError`] failures.
pub type CouchResult<T> = Result<T, CouchDaoError>;

/// Failures that can occur while interacting with CouchDB.
#[derive(Debug, Error)]
pub enum CouchDaoError {
    /// Required environment variable is missing.
    #[error("missing CouchDB environment variable `{var}`")]
    MissingEnvVar { var: &'static str },
    /// Building the HTTP client failed (invalid TLS setup, etc).
    #[error("failed to build CouchDB client")]
    ClientBuilder {
        #[source]
        source: reqwest::Error,
    },
    /// CouchDB rejected a GET against the target database.
    #[error("failed to query CouchDB database `{database}`")]
    DatabaseQuery {
        database: String,
        #[source]
        source: reqwest::Error,
    },
    /// CouchDB rejected a database creation request.
    #[error("failed to create CouchDB database `{database}`")]
    DatabaseCreate {
        database: String,
        #[source]
        source: reqwest::Error,
    },
    /// CouchDB returned an unexpected status code for a database operation.
    #[error("unexpected CouchDB database response status {status} for `{database}`")]
    DatabaseStatus {
        database: String,
        status: StatusCode,
    },
    /// A request to a document endpoint could not be sent.
    #[error("failed to send CouchDB request to `{path}`")]
    RequestSend {
        path: String,
        #[source]
        source: reqwest::Error,
    },
    /// CouchDB returned an unexpected status code for a document endpoint.
    #[error("unexpected CouchDB response status {status} for `{path}`")]
    RequestStatus { path: String, status: StatusCode },
    /// Response payload could not be parsed into JSON.
    #[error("failed to decode CouchDB response for `{path}`")]
    DecodeResponse {
        path: String,
        #[source]
        source: reqwest::Error,
    },
    /// Decoding a JSON value into the expected model failed.
    #[error("failed to deserialize CouchDB value for `{path}`")]
    DeserializeValue {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    /// Some required team documents are missing.
    #[error("missing team documents for game `{game_id}`, team IDs: {team_ids:?}")]
    MissingTeams {
        game_id: String,
        team_ids: Vec<uuid::Uuid>,
    },
    /// Failed to parse a document ID into UUIDs.
    #[error("invalid document ID `{doc_id}`: {kind}")]
    InvalidDocId { doc_id: String, kind: &'static str },
}
