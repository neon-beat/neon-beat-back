use mongodb::options::ClientOptions;

use super::error::{MongoDaoError, MongoResult};

#[derive(Clone)]
pub struct MongoConfig {
    pub options: ClientOptions,
    pub database_name: String,
}

impl MongoConfig {
    pub async fn from_uri(uri: &str, db_name: Option<&str>) -> MongoResult<Self> {
        let database_name = db_name.unwrap_or("neon_beat").to_owned();
        let options =
            ClientOptions::parse(uri)
                .await
                .map_err(|source| MongoDaoError::InvalidUri {
                    uri: uri.to_owned(),
                    source,
                })?;

        Ok(Self {
            options,
            database_name,
        })
    }
}
