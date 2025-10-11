#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CouchConfig {
    pub base_url: String,
    pub database: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[allow(dead_code)]
impl CouchConfig {
    pub fn new(base_url: impl Into<String>, database: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            database: database.into(),
            username: None,
            password: None,
        }
    }

    pub fn with_credentials(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.username = Some(username.into());
        self.password = Some(password.into());
        self
    }
}
