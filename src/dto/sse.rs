use serde::Serialize;
use utoipa::ToSchema;

#[derive(Clone, Debug)]
/// Dispatched payload carried across SSE channels.
pub struct ServerEvent {
    pub event: Option<String>,
    pub data: String,
}

impl ServerEvent {
    pub fn new<E, D>(event: E, data: D) -> Self
    where
        E: Into<Option<String>>,
        D: Into<String>,
    {
        Self {
            event: event.into(),
            data: data.into(),
        }
    }

    pub fn json<E, T>(event: E, payload: &T) -> serde_json::Result<Self>
    where
        E: Into<Option<String>>,
        T: Serialize,
    {
        Ok(Self {
            event: event.into(),
            data: serde_json::to_string(payload)?,
        })
    }
}

#[derive(Debug, Serialize, ToSchema)]
/// Token payload broadcast to newly connected admins.
pub struct AdminHandshake {
    pub token: String,
}
