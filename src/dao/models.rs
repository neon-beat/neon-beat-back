use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use uuid::Uuid;

/// Questions sequence definition containing a list of questions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuestionsSequenceEntity {
    /// Stable identifier for the questions sequence.
    pub id: Uuid,
    /// Human readable sequence name.
    pub name: String,
    /// Questions that make up the game.
    pub questions: Vec<QuestionEntity>,
}

/// Question entry inside a questions sequence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QuestionEntity {
    /// Music blindtest question.
    BlindTest(BlindTestQuestionEntity),
    /// Multiple-choice text question.
    MultipleChoice(MultipleChoiceQuestionEntity),
    /// Open text question.
    Open(OpenQuestionEntity),
}

/// Blindtest question entry inside a questions sequence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlindTestQuestionEntity {
    /// Timestamp (milliseconds) where the media preview should start.
    pub starts_at_ms: usize,
    /// Allowed time (milliseconds) for teams to answer.
    pub guess_duration_ms: usize,
    /// URL pointing to the media resource.
    pub url: String,
    /// Answers in import order. Runtime/API layers assign zero-based answer identifiers.
    pub answers: Vec<BlindTestAnswerEntity>,
}

/// Multiple-choice question entry inside a questions sequence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MultipleChoiceQuestionEntity {
    /// Allowed time (milliseconds) for teams to answer.
    pub guess_duration_ms: usize,
    /// Text prompt displayed to participants.
    pub prompt: String,
    /// Optional URL pointing to a supporting resource.
    pub url: Option<String>,
    /// Possible answers in import order. Runtime/API layers assign zero-based answer identifiers.
    pub answers: Vec<MultipleChoiceAnswerEntity>,
    /// Hints in import order. Runtime/API layers assign zero-based hint identifiers.
    pub hints: Vec<HintEntity>,
}

/// Open question entry inside a questions sequence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenQuestionEntity {
    /// Allowed time (milliseconds) for teams to answer.
    pub guess_duration_ms: usize,
    /// Text prompt displayed to participants.
    pub prompt: String,
    /// Optional URL pointing to a supporting resource.
    pub url: Option<String>,
    /// Accepted answers in import order. Runtime/API layers assign zero-based answer identifiers.
    pub answers: Vec<OpenAnswerEntity>,
    /// Hints in import order. Runtime/API layers assign zero-based hint identifiers.
    pub hints: Vec<HintEntity>,
}

/// Data for an answer associated to a blindtest question.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlindTestAnswerEntity {
    /// The name of the answer field (e.g. "Artist").
    pub key: String,
    /// The answer value for this field (e.g. the actual artist name).
    pub value: String,
    /// Points awarded for finding this answer.
    pub points: u8,
    /// Whether this answer is a bonus answer.
    pub is_bonus: bool,
}

/// Data for an answer associated to a multiple-choice question.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MultipleChoiceAnswerEntity {
    /// Answer text.
    pub text: String,
    /// Whether this answer is correct.
    pub is_correct: bool,
}

/// Data for an answer associated to an open question.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenAnswerEntity {
    /// Accepted answer text.
    pub text: String,
}

/// Hint text associated to a question.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HintEntity {
    /// Hint text.
    pub text: String,
}

/// Representation of a team stored in persistence and shared across layers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamEntity {
    /// Stable identifier for the team.
    pub id: Uuid,
    /// Display name chosen for the team.
    pub name: String,
    /// Current score for the team.
    pub score: i32,
    /// HSV color assigned to the team.
    pub color: TeamColorEntity,
    /// Last time this team was updated.
    pub updated_at: SystemTime,
}

/// HSV color representation for a team.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamColorEntity {
    /// Hue component (0.0 - 360.0).
    pub h: f32,
    /// Saturation component (0.0 - 1.0).
    pub s: f32,
    /// Value/brightness component (0.0 - 1.0).
    pub v: f32,
}

impl PartialEq for TeamColorEntity {
    fn eq(&self, other: &Self) -> bool {
        self.h.to_bits() == other.h.to_bits()
            && self.s.to_bits() == other.s.to_bits()
            && self.v.to_bits() == other.v.to_bits()
    }
}

impl Eq for TeamColorEntity {}

/// Summary representation of a team stored in persistence and shared across layers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TeamSummaryEntity {
    /// Stable identifier for the team.
    pub id: Uuid,
    /// Display name chosen for the team.
    pub name: String,
}

/// Aggregate game entity persisted by the storage layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameEntity {
    /// Primary key of the game.
    pub id: Uuid,
    /// Display name of the quiz / round.
    pub name: String,
    /// Creation timestamp for auditing/debugging.
    pub created_at: SystemTime,
    /// Last time the game entity was updated.
    pub updated_at: SystemTime,
    /// Participating teams and their current scores.
    pub teams: Vec<TeamEntity>,
    /// ID of the questions sequence used in this game session.
    pub questions_sequence_id: Uuid,
    /// Ordered list of question IDs from the sequence, defining the game order.
    pub question_order: Vec<u32>,
    /// Index of the current question to be played.
    pub current_question_index: Option<usize>,
    /// Whether the current question has already been revealed.
    pub current_question_revealed: bool,
}

/// Aggregate game list item entity (subset of GameEntity) persisted by the storage layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameListItemEntity {
    /// Primary key of the game.
    pub id: Uuid,
    /// Display name of the quiz / round.
    pub name: String,
    /// Creation timestamp for auditing/debugging.
    pub created_at: SystemTime,
    /// Last time the game entity was updated.
    pub updated_at: SystemTime,
    /// Participating teams.
    pub teams: Vec<TeamSummaryEntity>,
    /// ID of the questions sequence used in this game session.
    pub questions_sequence_id: Uuid,
}

impl From<TeamEntity> for TeamSummaryEntity {
    fn from(value: TeamEntity) -> Self {
        Self {
            id: value.id,
            name: value.name,
        }
    }
}

impl From<GameEntity> for GameListItemEntity {
    fn from(entity: GameEntity) -> Self {
        Self {
            id: entity.id,
            name: entity.name,
            created_at: entity.created_at,
            updated_at: entity.updated_at,
            teams: entity.teams.into_iter().map(Into::into).collect(),
            questions_sequence_id: entity.questions_sequence_id,
        }
    }
}
