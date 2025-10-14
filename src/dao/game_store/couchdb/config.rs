use super::error::{CouchDaoError, CouchResult};

/// Runtime configuration describing how to connect to CouchDB.
#[derive(Debug, Clone)]
pub struct CouchConfig {
    pub base_url: String,
    pub database: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl CouchConfig {
    /// Construct a configuration from explicit base URL and database name.
    pub fn new(base_url: impl Into<String>, database: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            database: database.into(),
            username: None,
            password: None,
        }
    }

    /// Attach basic-auth credentials to the configuration.
    pub fn with_credentials(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.username = Some(username.into());
        self.password = Some(password.into());
        self
    }

    /// Build a configuration by reading the expected environment variables.
    pub fn from_env() -> CouchResult<Self> {
        let base_url =
            std::env::var("COUCH_BASE_URL").map_err(|_| CouchDaoError::MissingEnvVar {
                var: "COUCH_BASE_URL",
            })?;
        let database = std::env::var("COUCH_DB")
            .map_err(|_| CouchDaoError::MissingEnvVar { var: "COUCH_DB" })?;

        let mut config = Self::new(base_url, database);

        if let (Some(username), Some(password)) = (
            std::env::var("COUCH_USERNAME").ok(),
            std::env::var("COUCH_PASSWORD").ok(),
        ) {
            config = config.with_credentials(username, password);
        }

        Ok(config)
    }
}
