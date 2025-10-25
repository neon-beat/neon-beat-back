use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::dto::common::TeamColorDto;

#[derive(Debug, Deserialize, ToSchema)]
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
/// Message emitted by the backend to drive LED patterns on a buzzer device.
pub struct BuzzerOutboundMessage {
    /// Visual pattern to display on the target buzzer.
    pub pattern: BuzzerPattern,
}

#[derive(Debug, Serialize, ToSchema)]
/// Available LED patterns that the buzzer firmware understands.
#[serde(tag = "type", content = "details", rename_all = "snake_case")]
pub enum BuzzerPattern {
    /// Blink pattern toggling between on/off.
    Blink(BuzzerPatternDetails),
    /// Switches the buzzer LEDs off.
    Off,
    /// Smooth continuous wave pattern.
    Wave(BuzzerPatternDetails),
}

#[derive(Debug, Serialize, ToSchema)]
/// Detailed settings for a LED pattern.
pub struct BuzzerPatternDetails {
    /// Duration of the effect in milliseconds (`0` means infinite).
    pub duration_ms: usize,
    /// Complete cycle length in milliseconds.
    pub period_ms: usize,
    /// Duty cycle expressed between `0.0` and `1.0`.
    pub dc: f32,
    /// HSV color used while the effect is active.
    pub color: TeamColorDto,
}
