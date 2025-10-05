use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, Serialize, ToSchema)]
/// Messages accepted from buzzer WebSocket clients.
#[serde(tag = "type")]
pub enum BuzzerInboundMessage {
    #[serde(rename = "identification")]
    Identification { id: String },
    #[serde(rename = "buzz")]
    Buzz { id: String },
    #[serde(other)]
    Unknown,
}

impl BuzzerInboundMessage {
    pub fn identification_id(&self) -> Option<&str> {
        match self {
            Self::Identification { id } => Some(id.as_str()),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
/// Positive acknowledgement sent to a buzzer after successful identification.
pub struct BuzzerAck {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
/// Feedback sent to a buzzer after it triggers a buzz event.
pub struct BuzzFeedback {
    pub id: String,
    pub can_answer: bool,
}
