#![allow(deprecated)]

use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::{Validate, ValidationErrors};

use crate::{
    dto::{common::TeamColorDto, format_system_time, validation::validate_buzzer_id},
    state::game::{
        BlindTestAnswer, BlindTestQuestion, GameSession, Hint, MultipleChoiceAnswer,
        MultipleChoiceQuestion, OpenAnswer, OpenQuestion, Question, QuestionsSequence, Team,
    },
};

/// Payload used to bootstrap a brand-new game instance with an inline questions sequence.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct CreateGameWithQuestionsSequenceRequest {
    /// Display name for the new game.
    pub name: String,
    /// List of teams participating in the game.
    #[validate(nested)]
    pub teams: Vec<TeamInput>,
    /// Questions sequence definition for the game.
    #[validate(nested)]
    pub questions_sequence: QuestionsSequenceInput,
}

/// Deprecated payload used to bootstrap a game with a legacy playlist.
#[deprecated(
    since = "0.9.0",
    note = "Deprecated legacy playlist compatibility. Use CreateGameWithQuestionsSequenceRequest and /admin/games/with-questions-sequence instead."
)]
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct LegacyCreateGameWithPlaylistRequest {
    /// Display name for the new game.
    pub name: String,
    /// List of teams participating in the game.
    #[validate(nested)]
    pub teams: Vec<TeamInput>,
    /// Legacy playlist definition for the game.
    #[validate(nested)]
    pub playlist: LegacyPlaylistInput,
}

impl From<LegacyCreateGameWithPlaylistRequest> for CreateGameWithQuestionsSequenceRequest {
    fn from(value: LegacyCreateGameWithPlaylistRequest) -> Self {
        Self {
            name: value.name,
            teams: value.teams,
            questions_sequence: value.playlist.into(),
        }
    }
}

/// Incoming team definition for the game bootstrap.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TeamInput {
    /// Display name for the team.
    pub name: String,
    /// If not specified, does not change it (or lets the back use the default value).
    /// If null is specified, removes the buzzer ID.
    /// If a string is specified, sets the buzzer ID to this string.
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub buzzer_id: Option<Option<String>>,
    /// Initial score for the team (defaults to 0 if omitted).
    #[serde(default)]
    #[schema(value_type = i32)]
    pub score: Option<i32>,
    /// Optional HSV color. If omitted, the backend chooses the first unused color from the
    /// configured colors set.
    #[serde(default)]
    #[schema(value_type = TeamColorDto)]
    pub color: Option<TeamColorDto>,
}

impl Validate for TeamInput {
    fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = ValidationErrors::new();

        if let Some(Some(ref id)) = self.buzzer_id {
            if let Err(e) = validate_buzzer_id(id) {
                errors.add("buzzer_id", e);
            }
        }

        if let Some(ref color) = self.color {
            if let Err(color_errors) = color.validate() {
                errors.merge_self("color", Err(color_errors));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

/// Questions sequence metadata and questions supplied when bootstrapping a game.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct QuestionsSequenceInput {
    /// Display name for the questions sequence.
    pub name: String,
    /// List of questions in the sequence.
    #[validate(nested)]
    pub questions: Vec<QuestionInput>,
}

/// Tagged question input.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QuestionInput {
    /// Blindtest question input.
    BlindTest(BlindTestQuestionInput),
    /// Multiple-choice question input.
    MultipleChoice(MultipleChoiceQuestionInput),
    /// Open text question input.
    Open(OpenQuestionInput),
}

impl Validate for QuestionInput {
    fn validate(&self) -> Result<(), ValidationErrors> {
        match self {
            Self::BlindTest(question) => question.validate(),
            Self::MultipleChoice(question) => question.validate(),
            Self::Open(question) => question.validate(),
        }
    }
}

/// Blindtest question details required to populate a sequence.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct BlindTestQuestionInput {
    /// Start time in milliseconds for media playback.
    pub starts_at_ms: usize,
    /// Duration in milliseconds for guessing.
    pub guess_duration_ms: usize,
    /// URL of the media file.
    #[validate(url)]
    pub url: String,
    /// Answers for this question.
    pub answers: Vec<BlindTestAnswerInput>,
}

/// Multiple-choice question details required to populate a sequence.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct MultipleChoiceQuestionInput {
    /// Duration in milliseconds for guessing.
    pub guess_duration_ms: usize,
    /// Text prompt displayed to participants.
    pub prompt: String,
    /// Optional URL of a supporting media file.
    #[validate(url)]
    pub url: Option<String>,
    /// Possible answers for this question.
    pub answers: Vec<MultipleChoiceAnswerInput>,
    /// Hints for this question.
    #[serde(default)]
    pub hints: Vec<String>,
}

/// Open question details required to populate a sequence.
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct OpenQuestionInput {
    /// Duration in milliseconds for guessing.
    pub guess_duration_ms: usize,
    /// Text prompt displayed to participants.
    pub prompt: String,
    /// Optional URL of a supporting media file.
    #[validate(url)]
    pub url: Option<String>,
    /// Accepted answers for this question.
    pub answers: Vec<OpenAnswerInput>,
    /// Hints for this question.
    #[serde(default)]
    pub hints: Vec<String>,
}

/// Blindtest answer details required for a question.
#[derive(Debug, Deserialize, ToSchema)]
pub struct BlindTestAnswerInput {
    /// Unique key identifying this answer field.
    pub key: String,
    /// The answer/value for this field.
    pub value: String,
    /// Points awarded for finding this answer.
    pub points: u8,
    /// Whether this answer is a bonus answer.
    #[serde(default)]
    pub is_bonus: bool,
}

/// Multiple-choice answer details required for a question.
#[derive(Debug, Deserialize, ToSchema)]
pub struct MultipleChoiceAnswerInput {
    /// Answer text.
    pub text: String,
    /// Whether this answer is correct.
    pub is_correct: bool,
}

/// Open answer details required for a question.
#[derive(Debug, Deserialize, ToSchema)]
pub struct OpenAnswerInput {
    /// Accepted answer text.
    pub text: String,
}

/// Deprecated playlist metadata supplied by legacy clients.
#[deprecated(
    since = "0.9.0",
    note = "Deprecated legacy playlist compatibility. Use QuestionsSequenceInput and /admin/questions-sequence instead."
)]
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct LegacyPlaylistInput {
    /// Display name for the playlist.
    pub name: String,
    /// List of songs in the playlist.
    #[validate(nested)]
    pub songs: Vec<LegacySongInput>,
}

/// Deprecated song details supplied by legacy clients.
#[deprecated(
    since = "0.9.0",
    note = "Deprecated legacy playlist compatibility. Use BlindTestQuestionInput inside QuestionInput instead."
)]
#[derive(Debug, Deserialize, ToSchema, Validate)]
pub struct LegacySongInput {
    /// Start time in milliseconds for the song playback.
    pub starts_at_ms: usize,
    /// Duration in milliseconds for guessing.
    pub guess_duration_ms: usize,
    /// URL of the song media file.
    #[validate(url)]
    pub url: String,
    /// Point fields for this song.
    pub point_fields: Vec<LegacyPointFieldInput>,
    /// Bonus fields for this song.
    #[serde(default)]
    pub bonus_fields: Vec<LegacyPointFieldInput>,
}

/// Deprecated point field details supplied by legacy clients.
#[deprecated(
    since = "0.9.0",
    note = "Deprecated legacy playlist compatibility. Use BlindTestAnswerInput instead."
)]
#[derive(Debug, Deserialize, ToSchema)]
pub struct LegacyPointFieldInput {
    /// Unique key identifying this field.
    pub key: String,
    /// The answer/value for this field.
    pub value: String,
    /// Points awarded for finding this field.
    pub points: u8,
}

impl From<LegacyPlaylistInput> for QuestionsSequenceInput {
    fn from(value: LegacyPlaylistInput) -> Self {
        Self {
            name: value.name,
            questions: value
                .songs
                .into_iter()
                .map(|song| {
                    let answers =
                        song.point_fields
                            .into_iter()
                            .map(|field| BlindTestAnswerInput {
                                key: field.key,
                                value: field.value,
                                points: field.points,
                                is_bonus: false,
                            })
                            .chain(song.bonus_fields.into_iter().map(|field| {
                                BlindTestAnswerInput {
                                    key: field.key,
                                    value: field.value,
                                    points: field.points,
                                    is_bonus: true,
                                }
                            }))
                            .collect();

                    QuestionInput::BlindTest(BlindTestQuestionInput {
                        starts_at_ms: song.starts_at_ms,
                        guess_duration_ms: song.guess_duration_ms,
                        url: song.url,
                        answers,
                    })
                })
                .collect(),
        }
    }
}

/// Summary returned once a game has been created or loaded.
#[derive(Debug, Serialize, ToSchema)]
pub struct GameSummary {
    /// Unique identifier for the game.
    pub id: String,
    /// Display name of the game.
    pub name: String,
    /// RFC3339 timestamp when the game was created.
    pub created_at: String,
    /// RFC3339 timestamp when the game was last updated.
    pub updated_at: String,
    /// List of teams in the game.
    pub teams: Vec<TeamSummary>,
    /// Summary of the questions sequence used in the game.
    pub questions_sequence: QuestionsSequenceSummary,
    /// Index of the current question being played (if any).
    pub current_question_index: Option<usize>,
}

/// Public projection of a team exposed to REST/SSE clients.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct TeamSummary {
    /// Unique identifier for the team.
    pub id: Uuid,
    /// ID of the buzzer assigned to this team.
    pub buzzer_id: Option<String>,
    /// Display name of the team.
    pub name: String,
    /// Current score for the team.
    pub score: i32,
    /// HSV color assigned to the team.
    pub color: TeamColorDto,
}

/// Brief team information without score or color.
#[derive(Debug, Serialize, ToSchema)]
pub struct TeamBriefSummary {
    /// Unique identifier for the team.
    pub id: Uuid,
    /// Display name of the team.
    pub name: String,
}

/// Summary of a questions sequence including all questions.
#[derive(Debug, Serialize, ToSchema)]
pub struct QuestionsSequenceSummary {
    /// Unique identifier for the questions sequence.
    pub id: Uuid,
    /// Display name of the questions sequence.
    pub name: String,
    /// List of questions in the sequence.
    pub questions: Vec<QuestionSummary>,
}

/// Summary of a single question within a sequence.
#[derive(Debug, Serialize, ToSchema, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QuestionSummary {
    /// Blindtest question summary.
    BlindTest(BlindTestQuestionSummary),
    /// Multiple-choice question summary.
    MultipleChoice(MultipleChoiceQuestionSummary),
    /// Open question summary.
    Open(OpenQuestionSummary),
}

/// Summary of a blindtest question.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct BlindTestQuestionSummary {
    /// Unique identifier for the question.
    pub id: u32,
    /// Start time in milliseconds for playback.
    pub starts_at_ms: usize,
    /// Duration in milliseconds for guessing.
    pub guess_duration_ms: usize,
    /// URL of the media file.
    pub url: String,
    /// Answers for this question.
    pub answers: HashMap<u8, BlindTestAnswerSummary>,
}

/// Summary of a multiple-choice question.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct MultipleChoiceQuestionSummary {
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
    pub answers: HashMap<u8, MultipleChoiceAnswerSummary>,
    /// Hints for this question.
    pub hints: HashMap<u8, HintSummary>,
}

/// Summary of an open question.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct OpenQuestionSummary {
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
    pub answers: HashMap<u8, OpenAnswerSummary>,
    /// Hints for this question.
    pub hints: HashMap<u8, HintSummary>,
}

/// Summary of a blindtest answer.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct BlindTestAnswerSummary {
    /// Unique key identifying this answer field.
    pub key: String,
    /// The answer/value for this field.
    pub value: String,
    /// Points awarded for finding this answer.
    pub points: u8,
    /// Whether this answer is a bonus answer.
    pub is_bonus: bool,
}

/// Summary of a multiple-choice answer.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct MultipleChoiceAnswerSummary {
    /// Answer text.
    pub text: String,
    /// Whether this answer is correct.
    pub is_correct: bool,
}

/// Summary of an open answer.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct OpenAnswerSummary {
    /// Accepted answer text.
    pub text: String,
}

/// Summary of a question hint.
#[derive(Debug, Serialize, ToSchema, Clone)]
pub struct HintSummary {
    /// Hint text.
    pub text: String,
}

/// Errors that can occur when validating question ordering.
#[derive(Debug, Error)]
pub enum QuestionOrderError {
    /// Question IDs in the order don't match the sequence questions.
    #[error("question ids mismatch (missing in order: {missing:?}, extra in order: {extra:?})")]
    MismatchedIds {
        /// Question IDs present in sequence but missing from order.
        missing: Vec<u32>,
        /// Question IDs present in order but not in sequence.
        extra: Vec<u32>,
    },
}

impl From<BlindTestAnswer> for BlindTestAnswerSummary {
    fn from(answer: BlindTestAnswer) -> Self {
        Self {
            key: answer.key,
            value: answer.value,
            points: answer.points,
            is_bonus: answer.is_bonus,
        }
    }
}

impl From<MultipleChoiceAnswer> for MultipleChoiceAnswerSummary {
    fn from(answer: MultipleChoiceAnswer) -> Self {
        Self {
            text: answer.text,
            is_correct: answer.is_correct,
        }
    }
}

impl From<OpenAnswer> for OpenAnswerSummary {
    fn from(answer: OpenAnswer) -> Self {
        Self { text: answer.text }
    }
}

impl From<Hint> for HintSummary {
    fn from(hint: Hint) -> Self {
        Self { text: hint.0 }
    }
}

impl From<(Uuid, Team)> for TeamSummary {
    fn from((id, team): (Uuid, Team)) -> Self {
        Self {
            id,
            buzzer_id: team.buzzer_id,
            name: team.name,
            score: team.score,
            color: team.color.into(),
        }
    }
}

impl From<(u32, Question)> for QuestionSummary {
    fn from((id, question): (u32, Question)) -> Self {
        match question {
            Question::BlindTest(question) => QuestionSummary::BlindTest((id, question).into()),
            Question::MultipleChoice(question) => {
                QuestionSummary::MultipleChoice((id, question).into())
            }
            Question::Open(question) => QuestionSummary::Open((id, question).into()),
        }
    }
}

impl From<(u32, BlindTestQuestion)> for BlindTestQuestionSummary {
    fn from((id, question): (u32, BlindTestQuestion)) -> Self {
        Self {
            id,
            starts_at_ms: question.starts_at_ms,
            guess_duration_ms: question.guess_duration_ms,
            url: question.url,
            answers: map_values(question.answers),
        }
    }
}

impl From<(u32, MultipleChoiceQuestion)> for MultipleChoiceQuestionSummary {
    fn from((id, question): (u32, MultipleChoiceQuestion)) -> Self {
        Self {
            id,
            guess_duration_ms: question.guess_duration_ms,
            prompt: question.prompt,
            url: question.url,
            answers: map_values(question.answers),
            hints: map_values(question.hints),
        }
    }
}

impl From<(u32, OpenQuestion)> for OpenQuestionSummary {
    fn from((id, question): (u32, OpenQuestion)) -> Self {
        Self {
            id,
            guess_duration_ms: question.guess_duration_ms,
            prompt: question.prompt,
            url: question.url,
            answers: map_values(question.answers),
            hints: map_values(question.hints),
        }
    }
}

impl TryFrom<(QuestionsSequence, Vec<u32>)> for QuestionsSequenceSummary {
    type Error = QuestionOrderError;

    fn try_from((sequence, order): (QuestionsSequence, Vec<u32>)) -> Result<Self, Self::Error> {
        let questions = ordered_question_summaries(sequence.questions, order)?;
        Ok(Self {
            id: sequence.id,
            name: sequence.name,
            questions,
        })
    }
}

impl TryFrom<GameSession> for GameSummary {
    type Error = QuestionOrderError;

    fn try_from(session: GameSession) -> Result<Self, Self::Error> {
        let questions_sequence_summary =
            (session.questions_sequence, session.question_order).try_into()?;

        Ok(Self {
            id: session.id.to_string(),
            name: session.name,
            created_at: format_system_time(session.created_at),
            updated_at: format_system_time(session.updated_at),
            teams: session.teams.into_iter().map(Into::into).collect(),
            questions_sequence: questions_sequence_summary,
            current_question_index: session.current_question_index,
        })
    }
}

fn ordered_question_summaries(
    sequence_questions: IndexMap<u32, Question>,
    order: Vec<u32>,
) -> Result<Vec<QuestionSummary>, QuestionOrderError> {
    let sequence_ids = sequence_questions.keys().cloned().collect::<HashSet<_>>();
    let order_ids = order.iter().copied().collect::<HashSet<_>>();

    if sequence_ids != order_ids {
        let mut missing = sequence_ids
            .difference(&order_ids)
            .copied()
            .collect::<Vec<_>>();
        let mut extra = order_ids
            .difference(&sequence_ids)
            .copied()
            .collect::<Vec<_>>();

        missing.sort_unstable();
        extra.sort_unstable();

        return Err(QuestionOrderError::MismatchedIds { missing, extra });
    }

    order
        .into_iter()
        .map(|question_id| {
            let Some(question_ref) = sequence_questions.get(&question_id) else {
                // Safety: mismatch should have been caught above, but guard defensively.
                return Err(QuestionOrderError::MismatchedIds {
                    missing: vec![question_id],
                    extra: Vec::new(),
                });
            };

            Ok((question_id, question_ref.clone()).into())
        })
        .collect::<Result<Vec<QuestionSummary>, _>>()
}

/// Convert all values in a keyed map while preserving their original keys.
fn map_values<K, V, O>(values: HashMap<K, V>) -> HashMap<K, O>
where
    K: Eq + Hash,
    V: Into<O>,
{
    values
        .into_iter()
        .map(|(id, value)| (id, value.into()))
        .collect()
}
