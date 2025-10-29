use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::{
    dto::{game::TeamSummary, phase::VisibleGamePhase},
    state::game::{PointField, Song, TeamColor},
};

/// Snapshot of a point field for DTO use.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct PointFieldSnapshot {
    /// Unique key identifying this field.
    pub key: String,
    /// The answer/value for this field.
    pub value: String,
    /// Points awarded for finding this field.
    pub points: u8,
}

impl From<PointField> for PointFieldSnapshot {
    fn from(field: PointField) -> Self {
        Self {
            key: field.key,
            value: field.value,
            points: field.points,
        }
    }
}

/// Snapshot of a song including point fields metadata.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct SongSnapshot {
    /// Unique identifier for the song.
    pub id: u32,
    /// Start time in milliseconds for playback.
    pub starts_at_ms: usize,
    /// Duration in milliseconds for guessing.
    pub guess_duration_ms: usize,
    /// URL of the song media file.
    pub url: String,
    /// Required point fields for this song.
    pub point_fields: Vec<PointFieldSnapshot>,
    /// Optional bonus fields for this song.
    pub bonus_fields: Vec<PointFieldSnapshot>,
}

impl SongSnapshot {
    /// Create a song snapshot from a game session song.
    pub fn from_game_song(id: u32, song: &Song) -> Self {
        Self {
            id,
            starts_at_ms: song.starts_at_ms,
            guess_duration_ms: song.guess_duration_ms,
            url: song.url.clone(),
            point_fields: song
                .point_fields
                .clone()
                .into_iter()
                .map(PointFieldSnapshot::from)
                .collect(),
            bonus_fields: song
                .bonus_fields
                .clone()
                .into_iter()
                .map(PointFieldSnapshot::from)
                .collect(),
        }
    }
}

/// Shared snapshot describing the current gameplay phase and related context.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct GamePhaseSnapshot {
    /// Current phase of the game.
    pub phase: VisibleGamePhase,
    /// ID of the active game (if any).
    pub game_id: Option<Uuid>,
    /// True when the backend operates in degraded mode (no connexion to database).
    pub degraded: bool,
    /// Present during prep_pairing phase to indicate the active team.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pairing_team_id: Option<Uuid>,
    /// Present during pause phase for buzz-induced pauses to expose the buzzer identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused_buzzer: Option<String>,
    /// Present during playing/reveal phases to expose the current song.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub song: Option<SongSnapshot>,
    /// Present during scores phase to display the final scores.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scoreboard: Option<Vec<TeamSummary>>,
    /// Present during playing/reveal phases to expose point fields already found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found_point_fields: Option<Vec<String>>,
    /// Present during playing/reveal phases to expose bonus fields already found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found_bonus_fields: Option<Vec<String>>,
}

/// HSV representation shared by DTOs (REST, SSE, WS).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, ToSchema, Validate)]
pub struct TeamColorDto {
    /// Hue component (degrees).
    pub h: f32,
    /// Saturation component (0.0 to 1.0).
    #[validate(range(min = 0.0, max = 1.0))]
    pub s: f32,
    /// Value (brightness) component (0.0 to 1.0).
    #[validate(range(min = 0.0, max = 1.0))]
    pub v: f32,
}

impl From<TeamColor> for TeamColorDto {
    fn from(color: TeamColor) -> Self {
        Self {
            h: color.h,
            s: color.s,
            v: color.v,
        }
    }
}

impl From<TeamColorDto> for TeamColor {
    fn from(color: TeamColorDto) -> Self {
        Self {
            h: color.h,
            s: color.s,
            v: color.v,
        }
    }
}
