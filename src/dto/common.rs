use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;
use validator::Validate;

use crate::{
    dto::{game::TeamSummary, phase::VisibleGamePhase},
    state::game::{
        BlindTestAnswer, BlindTestQuestion, Hint, MultipleChoiceAnswer, MultipleChoiceQuestion,
        OpenAnswer, OpenQuestion, Question, TeamColor,
    },
};

/// Snapshot of a question including answer metadata.
#[derive(Debug, Serialize, ToSchema, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QuestionSnapshot {
    /// Blindtest question snapshot.
    BlindTest(BlindTestQuestionSnapshot),
    /// Multiple-choice question snapshot.
    MultipleChoice(MultipleChoiceQuestionSnapshot),
    /// Open question snapshot.
    Open(OpenQuestionSnapshot),
}

/// Snapshot of a blindtest question exposed to public runtime consumers.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct BlindTestQuestionSnapshot {
    /// Unique identifier for the question.
    pub id: u32,
    /// Start time in milliseconds for playback.
    pub starts_at_ms: usize,
    /// Duration in milliseconds for guessing.
    pub guess_duration_ms: usize,
    /// URL of the media file.
    pub url: String,
    /// Answers for this question.
    pub answers: HashMap<u8, BlindTestAnswerSnapshot>,
}

/// Snapshot of a multiple-choice question exposed to public runtime consumers.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct MultipleChoiceQuestionSnapshot {
    /// Unique identifier for the question.
    pub id: u32,
    /// Duration in milliseconds for guessing.
    pub guess_duration_ms: usize,
    /// Text prompt displayed to participants.
    pub prompt: String,
    /// Optional URL of a supporting media file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Possible answers for this question.
    pub answers: HashMap<u8, MultipleChoiceAnswerSnapshot>,
    /// Hints for this question.
    pub hints: HashMap<u8, HintSnapshot>,
}

/// Snapshot of an open question exposed to public runtime consumers.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct OpenQuestionSnapshot {
    /// Unique identifier for the question.
    pub id: u32,
    /// Duration in milliseconds for guessing.
    pub guess_duration_ms: usize,
    /// Text prompt displayed to participants.
    pub prompt: String,
    /// Optional URL of a supporting media file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Accepted answers for this question.
    pub answers: HashMap<u8, OpenAnswerSnapshot>,
    /// Hints for this question.
    pub hints: HashMap<u8, HintSnapshot>,
}

/// Snapshot of a blindtest answer exposed to public runtime consumers.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct BlindTestAnswerSnapshot {
    /// Unique key identifying this answer field.
    pub key: String,
    /// The answer/value for this field.
    pub value: String,
    /// Points awarded for finding this answer.
    pub points: u8,
    /// Whether this answer is a bonus answer.
    pub is_bonus: bool,
}

/// Snapshot of a multiple-choice answer exposed to public runtime consumers.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct MultipleChoiceAnswerSnapshot {
    /// Answer text.
    pub text: String,
    /// Whether this answer is correct.
    pub is_correct: bool,
}

/// Snapshot of an open answer exposed to public runtime consumers.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct OpenAnswerSnapshot {
    /// Accepted answer text.
    pub text: String,
}

/// Snapshot of a question hint exposed to public runtime consumers.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct HintSnapshot {
    /// Hint text.
    pub text: String,
}

impl QuestionSnapshot {
    /// Create a question snapshot from a game session question.
    pub fn from_game_question(id: u32, question: &Question) -> Self {
        match question {
            Question::BlindTest(question) => Self::BlindTest((id, question).into()),
            Question::MultipleChoice(question) => Self::MultipleChoice((id, question).into()),
            Question::Open(question) => Self::Open((id, question).into()),
        }
    }
}

impl From<(u32, &BlindTestQuestion)> for BlindTestQuestionSnapshot {
    fn from((id, question): (u32, &BlindTestQuestion)) -> Self {
        Self {
            id,
            starts_at_ms: question.starts_at_ms,
            guess_duration_ms: question.guess_duration_ms,
            url: question.url.clone(),
            answers: map_snapshot_values(&question.answers),
        }
    }
}

impl From<(u32, &MultipleChoiceQuestion)> for MultipleChoiceQuestionSnapshot {
    fn from((id, question): (u32, &MultipleChoiceQuestion)) -> Self {
        Self {
            id,
            guess_duration_ms: question.guess_duration_ms,
            prompt: question.prompt.clone(),
            url: question.url.clone(),
            answers: map_snapshot_values(&question.answers),
            hints: map_snapshot_values(&question.hints),
        }
    }
}

impl From<(u32, &OpenQuestion)> for OpenQuestionSnapshot {
    fn from((id, question): (u32, &OpenQuestion)) -> Self {
        Self {
            id,
            guess_duration_ms: question.guess_duration_ms,
            prompt: question.prompt.clone(),
            url: question.url.clone(),
            answers: map_snapshot_values(&question.answers),
            hints: map_snapshot_values(&question.hints),
        }
    }
}

impl From<&BlindTestAnswer> for BlindTestAnswerSnapshot {
    fn from(answer: &BlindTestAnswer) -> Self {
        Self {
            key: answer.key.clone(),
            value: answer.value.clone(),
            points: answer.points,
            is_bonus: answer.is_bonus,
        }
    }
}

impl From<&MultipleChoiceAnswer> for MultipleChoiceAnswerSnapshot {
    fn from(answer: &MultipleChoiceAnswer) -> Self {
        Self {
            text: answer.text.clone(),
            is_correct: answer.is_correct,
        }
    }
}

impl From<&OpenAnswer> for OpenAnswerSnapshot {
    fn from(answer: &OpenAnswer) -> Self {
        Self {
            text: answer.text.clone(),
        }
    }
}

impl From<&Hint> for HintSnapshot {
    fn from(hint: &Hint) -> Self {
        Self {
            text: hint.0.clone(),
        }
    }
}

/// Convert a map of runtime values into their public snapshot DTO equivalents.
fn map_snapshot_values<V, O>(values: &HashMap<u8, V>) -> HashMap<u8, O>
where
    for<'a> O: From<&'a V>,
{
    values
        .iter()
        .map(|(id, value)| (*id, O::from(value)))
        .collect()
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
    /// Present during playing/reveal phases to expose the current question.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<QuestionSnapshot>,
    /// Present during scores phase to display the final scores.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scoreboard: Option<Vec<TeamSummary>>,
    /// Present during playing/reveal phases to expose answer IDs already found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answers_ids: Option<Vec<u8>>,
    /// Present during playing/reveal phases to expose hint IDs already revealed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hints_ids: Option<Vec<u8>>,
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
