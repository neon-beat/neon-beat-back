use serde::Serialize;
use utoipa::ToSchema;

#[derive(Clone, Debug)]
/// Dispatched payload carried across SSE channels.
pub struct ServerEvent {
    pub event: Option<String>,
    pub data: String,
}

impl ServerEvent {
    /// Build a raw SSE event with pre-serialised content.
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

    /// Convenience wrapper that serialises `payload` into the SSE data field.
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
/// Initial metadata sent to an SSE client when it connects.
pub struct Handshake {
    /// Identifier of the SSE stream (`public` or `admin`).
    pub stream: String,
    /// Human-readable message confirming the subscription.
    pub message: String,
    /// Whether the backend is running without a MongoDB connection.
    pub degraded: bool,
    /// Optional admin token returned when the stream is privileged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
/// Broadcast when the backend enters or leaves degraded mode.
pub struct SystemStatus {
    pub degraded: bool,
}
