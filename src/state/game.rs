use indexmap::IndexMap;
use rand::{rng, seq::SliceRandom};
use std::{collections::HashMap, time::SystemTime};
use uuid::Uuid;

use crate::{
    dao::models::{
        BlindTestAnswerEntity, BlindTestQuestionEntity, GameEntity, HintEntity,
        MultipleChoiceAnswerEntity, MultipleChoiceQuestionEntity, OpenAnswerEntity,
        OpenQuestionEntity, QuestionEntity, QuestionsSequenceEntity, TeamColorEntity, TeamEntity,
        TeamSummaryEntity,
    },
    dto::game::TeamBriefSummary,
};

/// Runtime representation of a questions sequence keyed by question identifier.
#[derive(Debug, Clone)]
pub struct QuestionsSequence {
    /// Stable identifier for the questions sequence.
    pub id: Uuid,
    /// Human readable sequence name.
    pub name: String,
    /// Set of questions that make up the game (key is the ID of the question).
    pub questions: IndexMap<u32, Question>,
}

/// Question supported by a game sequence.
#[derive(Debug, Clone)]
pub enum Question {
    /// Music blindtest question.
    BlindTest(BlindTestQuestion),
    /// Multiple-choice text question.
    MultipleChoice(MultipleChoiceQuestion),
    /// Open text question.
    Open(OpenQuestion),
}

/// Metadata for a blindtest question.
#[derive(Debug, Clone)]
pub struct BlindTestQuestion {
    /// Timestamp (milliseconds) where the media preview should start.
    pub starts_at_ms: usize,
    /// Allowed time (milliseconds) for teams to answer.
    pub guess_duration_ms: usize,
    /// URL pointing to the media resource.
    pub url: String,
    /// Answers for this question keyed by auto-assigned answer identifier.
    pub answers: HashMap<u8, BlindTestAnswer>,
}

/// Metadata for a multiple-choice question.
#[derive(Debug, Clone)]
pub struct MultipleChoiceQuestion {
    /// Allowed time (milliseconds) for teams to answer.
    pub guess_duration_ms: usize,
    /// Text prompt displayed to participants.
    pub prompt: String,
    /// Optional URL pointing to a supporting resource.
    pub url: Option<String>,
    /// Possible answers keyed by auto-assigned answer identifier.
    pub answers: HashMap<u8, MultipleChoiceAnswer>,
    /// Hints keyed by auto-assigned hint identifier.
    pub hints: HashMap<u8, Hint>,
}

/// Metadata for an open text question.
#[derive(Debug, Clone)]
pub struct OpenQuestion {
    /// Allowed time (milliseconds) for teams to answer.
    pub guess_duration_ms: usize,
    /// Text prompt displayed to participants.
    pub prompt: String,
    /// Optional URL pointing to a supporting resource.
    pub url: Option<String>,
    /// Accepted answers keyed by auto-assigned answer identifier.
    pub answers: HashMap<u8, OpenAnswer>,
    /// Hints keyed by auto-assigned hint identifier.
    pub hints: HashMap<u8, Hint>,
}

/// Data for an answer associated to a blindtest question.
#[derive(Debug, Clone)]
pub struct BlindTestAnswer {
    /// The name of the field to find (e.g. "Artist").
    pub key: String,
    /// The value to find for this field (e.g. the actual artist name).
    pub value: String,
    /// The number of points awarded if this answer is found.
    pub points: u8,
    /// Whether this answer is a bonus answer.
    pub is_bonus: bool,
}

/// Data for an answer associated to a multiple-choice question.
#[derive(Debug, Clone)]
pub struct MultipleChoiceAnswer {
    /// Answer text.
    pub text: String,
    /// Whether this answer is correct.
    pub is_correct: bool,
}

/// Data for an answer associated to an open question.
#[derive(Debug, Clone)]
pub struct OpenAnswer {
    /// Accepted answer text.
    pub text: String,
}

/// Hint text associated to a question.
#[derive(Debug, Clone)]
pub struct Hint(pub String);

impl Question {
    /// Return true when the question contains the requested answer identifier.
    pub fn has_answer(&self, answer_id: u8) -> bool {
        match self {
            Question::BlindTest(question) => question.answers.contains_key(&answer_id),
            Question::MultipleChoice(question) => question.answers.contains_key(&answer_id),
            Question::Open(question) => question.answers.contains_key(&answer_id),
        }
    }

    /// Return true when the question contains the requested hint identifier.
    ///
    /// Blindtest questions use the answer map as their hint source.
    pub fn has_hint(&self, hint_id: u8) -> bool {
        match self {
            Question::BlindTest(question) => question.answers.contains_key(&hint_id),
            Question::MultipleChoice(question) => question.hints.contains_key(&hint_id),
            Question::Open(question) => question.hints.contains_key(&hint_id),
        }
    }
}

/// HSV color assigned to a team.
#[derive(Debug, Clone)]
pub struct TeamColor {
    /// Hue component (in degrees) of the HSV color.
    pub h: f32,
    /// Saturation component of the HSV color.
    pub s: f32,
    /// Value (brightness) component of the HSV color.
    pub v: f32,
}

impl PartialEq for TeamColor {
    fn eq(&self, other: &Self) -> bool {
        self.h.to_bits() == other.h.to_bits()
            && self.s.to_bits() == other.s.to_bits()
            && self.v.to_bits() == other.v.to_bits()
    }
}

impl Eq for TeamColor {}

/// Team info tracked during a game session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Team {
    /// Unique buzzer identifier (12 lowercase hexadecimal characters).
    pub buzzer_id: Option<String>,
    /// Display name chosen for the team.
    pub name: String,
    /// Current score for the team.
    pub score: i32,
    /// HSV color assigned to the team.
    pub color: TeamColor,
    /// Timestamp of the last update to this team.
    pub updated_at: SystemTime,
}

/// Aggregated state for an in-progress or persisted game session.
#[derive(Debug, Clone)]
pub struct GameSession {
    /// Primary key of the game.
    pub id: Uuid,
    /// Display name of the quiz / round.
    pub name: String,
    /// Creation timestamp for auditing/debugging.
    pub created_at: SystemTime,
    /// Last time the game document was updated.
    pub updated_at: SystemTime,
    /// Participating teams and their current scores keyed by team identifier.
    pub teams: IndexMap<Uuid, Team>,
    /// Questions sequence selected for this session.
    pub questions_sequence: QuestionsSequence,
    /// Ordered list of question IDs from the sequence, defining the game order.
    pub question_order: Vec<u32>,
    /// Index of the current question to be played.
    pub current_question_index: Option<usize>,
    /// Whether the current question has already been revealed.
    pub current_question_revealed: bool,
    /// Answer IDs already found for the current question.
    pub found_answer_ids: Vec<u8>,
    /// Hint IDs already revealed for the current question.
    pub revealed_hint_ids: Vec<u8>,
}

impl GameSession {
    /// Build a new in-memory session with the provided metadata.
    pub fn new(
        name: String,
        teams: IndexMap<Uuid, Team>,
        questions_sequence: QuestionsSequence,
        shuffle_questions: bool,
    ) -> Self {
        let timestamp = SystemTime::now();

        let mut question_order: Vec<u32> = questions_sequence.questions.keys().cloned().collect();
        if shuffle_questions {
            let mut rng = rng();
            question_order.shuffle(&mut rng);
        }

        Self {
            id: Uuid::new_v4(),
            name,
            created_at: timestamp,
            updated_at: timestamp,
            teams,
            questions_sequence,
            question_order,
            current_question_index: Some(0),
            current_question_revealed: false,
            found_answer_ids: Vec::new(),
            revealed_hint_ids: Vec::new(),
        }
    }

    /// Return the question at the requested sequence index together with its identifier.
    pub fn get_question(&self, index: usize) -> Option<(u32, Question)> {
        self.question_order.get(index).and_then(|question_id| {
            self.questions_sequence
                .questions
                .get(question_id)
                .map(|question| (*question_id, question.clone()))
        })
    }

    /// Insert a new team into the session, generating default values when they are omitted.
    ///
    /// The color is selected from the configured colors set when not specified and the team name
    /// falls back to `Team X` (with X starting at 1) to keep the UI human-friendly.
    pub fn add_team(
        &mut self,
        config: &crate::config::AppConfig,
        name: Option<String>,
        buzzer_id: Option<String>,
        score: Option<i32>,
        color: Option<TeamColor>,
    ) -> (Uuid, Team) {
        let team_id = Uuid::new_v4();
        // Reuse provided color when present, otherwise pick the next free colors set slot.
        let color = color.unwrap_or_else(|| {
            config.first_unused_color(
                &self
                    .teams
                    .values()
                    .map(|existing| existing.color.clone())
                    .collect::<Vec<_>>(),
            )
        });
        let team = Team {
            buzzer_id,
            name: name.unwrap_or_else(|| format!("Team {}", self.teams.len() + 1)),
            score: score.unwrap_or(0),
            color,
            updated_at: SystemTime::now(),
        };
        self.teams.insert(team_id, team.clone());
        (team_id, team)
    }
}

impl QuestionsSequence {
    /// Build a new in-memory questions sequence with the provided metadata, allocating a
    /// fresh unique identifier for runtime usage.
    pub fn new(name: String, questions: IndexMap<u32, Question>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            questions,
        }
    }
}

impl From<BlindTestAnswerEntity> for BlindTestAnswer {
    fn from(value: BlindTestAnswerEntity) -> Self {
        Self {
            key: value.key,
            value: value.value,
            points: value.points,
            is_bonus: value.is_bonus,
        }
    }
}

impl From<BlindTestAnswer> for BlindTestAnswerEntity {
    fn from(value: BlindTestAnswer) -> Self {
        Self {
            key: value.key,
            value: value.value,
            points: value.points,
            is_bonus: value.is_bonus,
        }
    }
}

impl From<MultipleChoiceAnswerEntity> for MultipleChoiceAnswer {
    fn from(value: MultipleChoiceAnswerEntity) -> Self {
        Self {
            text: value.text,
            is_correct: value.is_correct,
        }
    }
}

impl From<MultipleChoiceAnswer> for MultipleChoiceAnswerEntity {
    fn from(value: MultipleChoiceAnswer) -> Self {
        Self {
            text: value.text,
            is_correct: value.is_correct,
        }
    }
}

impl From<OpenAnswerEntity> for OpenAnswer {
    fn from(value: OpenAnswerEntity) -> Self {
        Self { text: value.text }
    }
}

impl From<OpenAnswer> for OpenAnswerEntity {
    fn from(value: OpenAnswer) -> Self {
        Self { text: value.text }
    }
}

impl From<HintEntity> for Hint {
    fn from(value: HintEntity) -> Self {
        Self(value.text)
    }
}

impl From<Hint> for HintEntity {
    fn from(value: Hint) -> Self {
        Self { text: value.0 }
    }
}

impl From<BlindTestQuestionEntity> for BlindTestQuestion {
    fn from(value: BlindTestQuestionEntity) -> Self {
        Self {
            starts_at_ms: value.starts_at_ms,
            guess_duration_ms: value.guess_duration_ms,
            url: value.url,
            answers: value
                .answers
                .into_iter()
                .enumerate()
                .map(|(id, answer)| (id as u8, answer.into()))
                .collect(),
        }
    }
}

impl From<BlindTestQuestion> for BlindTestQuestionEntity {
    fn from(value: BlindTestQuestion) -> Self {
        Self {
            starts_at_ms: value.starts_at_ms,
            guess_duration_ms: value.guess_duration_ms,
            url: value.url,
            answers: value.answers.into_values().map(Into::into).collect(),
        }
    }
}

impl From<MultipleChoiceQuestionEntity> for MultipleChoiceQuestion {
    fn from(value: MultipleChoiceQuestionEntity) -> Self {
        Self {
            guess_duration_ms: value.guess_duration_ms,
            prompt: value.prompt,
            url: value.url,
            answers: value
                .answers
                .into_iter()
                .enumerate()
                .map(|(id, answer)| (id as u8, answer.into()))
                .collect(),
            hints: value
                .hints
                .into_iter()
                .enumerate()
                .map(|(id, hint)| (id as u8, hint.into()))
                .collect(),
        }
    }
}

impl From<MultipleChoiceQuestion> for MultipleChoiceQuestionEntity {
    fn from(value: MultipleChoiceQuestion) -> Self {
        Self {
            guess_duration_ms: value.guess_duration_ms,
            prompt: value.prompt,
            url: value.url,
            answers: value.answers.into_values().map(Into::into).collect(),
            hints: value.hints.into_values().map(Into::into).collect(),
        }
    }
}

impl From<OpenQuestionEntity> for OpenQuestion {
    fn from(value: OpenQuestionEntity) -> Self {
        Self {
            guess_duration_ms: value.guess_duration_ms,
            prompt: value.prompt,
            url: value.url,
            answers: value
                .answers
                .into_iter()
                .enumerate()
                .map(|(id, answer)| (id as u8, answer.into()))
                .collect(),
            hints: value
                .hints
                .into_iter()
                .enumerate()
                .map(|(id, hint)| (id as u8, hint.into()))
                .collect(),
        }
    }
}

impl From<OpenQuestion> for OpenQuestionEntity {
    fn from(value: OpenQuestion) -> Self {
        Self {
            guess_duration_ms: value.guess_duration_ms,
            prompt: value.prompt,
            url: value.url,
            answers: value.answers.into_values().map(Into::into).collect(),
            hints: value.hints.into_values().map(Into::into).collect(),
        }
    }
}

impl From<QuestionEntity> for Question {
    fn from(value: QuestionEntity) -> Self {
        match value {
            QuestionEntity::BlindTest(question) => Question::BlindTest(question.into()),
            QuestionEntity::MultipleChoice(question) => Question::MultipleChoice(question.into()),
            QuestionEntity::Open(question) => Question::Open(question.into()),
        }
    }
}

impl From<Question> for QuestionEntity {
    fn from(value: Question) -> Self {
        match value {
            Question::BlindTest(question) => QuestionEntity::BlindTest(question.into()),
            Question::MultipleChoice(question) => QuestionEntity::MultipleChoice(question.into()),
            Question::Open(question) => QuestionEntity::Open(question.into()),
        }
    }
}

impl From<QuestionsSequenceEntity> for QuestionsSequence {
    fn from(value: QuestionsSequenceEntity) -> Self {
        Self {
            id: value.id,
            name: value.name,
            questions: value
                .questions
                .into_iter()
                .enumerate()
                .map(|(id, question)| (id as u32, question.into()))
                .collect(),
        }
    }
}

impl From<QuestionsSequence> for QuestionsSequenceEntity {
    fn from(value: QuestionsSequence) -> Self {
        Self {
            id: value.id,
            name: value.name,
            questions: value.questions.into_values().map(Into::into).collect(),
        }
    }
}

impl From<TeamEntity> for (Uuid, Team) {
    fn from(value: TeamEntity) -> Self {
        let id = value.id;
        let team = Team {
            buzzer_id: None,
            name: value.name,
            score: value.score,
            color: value.color.into(),
            updated_at: value.updated_at,
        };
        (id, team)
    }
}

impl From<(Uuid, Team)> for TeamEntity {
    fn from((id, team): (Uuid, Team)) -> Self {
        Self {
            id,
            name: team.name,
            score: team.score,
            color: team.color.into(),
            updated_at: team.updated_at,
        }
    }
}

impl From<TeamSummaryEntity> for TeamBriefSummary {
    fn from(value: TeamSummaryEntity) -> Self {
        Self {
            id: value.id,
            name: value.name,
        }
    }
}

impl From<TeamColorEntity> for TeamColor {
    fn from(value: TeamColorEntity) -> Self {
        Self {
            h: value.h,
            s: value.s,
            v: value.v,
        }
    }
}

impl From<TeamColor> for TeamColorEntity {
    fn from(value: TeamColor) -> Self {
        Self {
            h: value.h,
            s: value.s,
            v: value.v,
        }
    }
}

impl From<(GameEntity, QuestionsSequenceEntity)> for GameSession {
    fn from((game, questions_sequence): (GameEntity, QuestionsSequenceEntity)) -> Self {
        Self {
            id: game.id,
            name: game.name,
            created_at: game.created_at,
            updated_at: game.updated_at,
            teams: game.teams.into_iter().map(Into::into).collect(),
            questions_sequence: questions_sequence.into(),
            question_order: game.question_order,
            current_question_index: game.current_question_index,
            current_question_revealed: game.current_question_revealed,
            found_answer_ids: Vec::new(),
            revealed_hint_ids: Vec::new(),
        }
    }
}

impl From<GameSession> for GameEntity {
    fn from(value: GameSession) -> Self {
        Self {
            id: value.id,
            name: value.name,
            created_at: value.created_at,
            updated_at: value.updated_at,
            teams: value.teams.into_iter().map(Into::into).collect(),
            questions_sequence_id: value.questions_sequence.id,
            question_order: value.question_order,
            current_question_index: value.current_question_index,
            current_question_revealed: value.current_question_revealed,
        }
    }
}
