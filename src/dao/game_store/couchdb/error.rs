use reqwest::StatusCode;
use thiserror::Error;

pub type CouchResult<T> = Result<T, CouchDaoError>;

#[derive(Debug, Error)]
pub enum CouchDaoError {
    #[error("missing CouchDB environment variable `{var}`")]
    MissingEnvVar { var: &'static str },
    #[error("failed to build CouchDB client")]
    ClientBuilder {
        #[source]
        source: reqwest::Error,
    },
    #[error("failed to query CouchDB database `{database}`")]
    DatabaseQuery {
        database: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("failed to create CouchDB database `{database}`")]
    DatabaseCreate {
        database: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("unexpected CouchDB database response status {status} for `{database}`")]
    DatabaseStatus {
        database: String,
        status: StatusCode,
    },
    #[error("failed to send CouchDB request to `{path}`")]
    RequestSend {
        path: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("unexpected CouchDB response status {status} for `{path}`")]
    RequestStatus { path: String, status: StatusCode },
    #[error("failed to decode CouchDB response for `{path}`")]
    DecodeResponse {
        path: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("failed to deserialize CouchDB value for `{path}`")]
    DeserializeValue {
        path: String,
        #[source]
        source: serde_json::Error,
    },
}
