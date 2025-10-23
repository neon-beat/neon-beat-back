use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    dto::{game::TeamSummary, phase::VisibleGamePhase},
    state::game::{PointField, Song},
};

/// Snapshot of a point field for DTO use.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct PointFieldSnapshot {
    pub key: String,
    pub value: String,
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
    pub id: u32,
    pub starts_at_ms: usize,
    pub guess_duration_ms: usize,
    pub url: String,
    pub point_fields: Vec<PointFieldSnapshot>,
    pub bonus_fields: Vec<PointFieldSnapshot>,
}

impl SongSnapshot {
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
    pub phase: VisibleGamePhase,
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
