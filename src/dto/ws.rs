use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::ValidationError;

use crate::dto::{common::TeamColorDto, validation::validate_buzzer_id};

#[derive(Debug, Deserialize, ToSchema)]
/// Messages accepted from buzzer WebSocket clients.
#[serde(tag = "type")]
pub enum BuzzerInboundMessage {
    #[serde(rename = "identification")]
    Identification { id: String },
    #[serde(rename = "buzz")]
    Buzz { id: String },
}

impl BuzzerInboundMessage {
    /// Deserialize and validate a buzzer message from JSON string.
    ///
    /// This combines deserialization and validation into a single operation,
    /// ensuring that the returned message is both well-formed and valid.
    /// Returns an error if the message type is unknown or validation fails.
    pub fn from_json_str(s: &str) -> Result<Self, BuzzerMessageError> {
        let msg: Self = serde_json::from_str(s)?;
        msg.validate()?;
        Ok(msg)
    }

    /// Validates the buzzer ID for Identification and Buzz messages.
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Identification { id } | Self::Buzz { id } => validate_buzzer_id(id),
        }
    }
}

/// Errors that can occur when parsing and validating buzzer messages.
#[derive(Debug, thiserror::Error)]
pub enum BuzzerMessageError {
    #[error("invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("validation failed: {0}")]
    ValidationFailed(#[from] ValidationError),
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
