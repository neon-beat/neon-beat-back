use std::{
    collections::{HashMap, HashSet},
    time::SystemTime,
};

use indexmap::IndexMap;
use rand::{rng, seq::SliceRandom};
use uuid::Uuid;

use crate::{
    config::AppConfig,
    dao::models::{GameEntity, QuestionsSequenceEntity},
    dto::game::{
        BlindTestAnswerInput, GameSummary, MultipleChoiceAnswerInput, OpenAnswerInput,
        QuestionInput, QuestionsSequenceInput, QuestionsSequenceSummary, TeamInput,
    },
    error::ServiceError,
    services::sse_events,
    state::{
        self, SharedState,
        game::{
            BlindTestAnswer, BlindTestQuestion, GameSession, Hint, MultipleChoiceAnswer,
            MultipleChoiceQuestion, OpenAnswer, OpenQuestion, Question, QuestionsSequence, Team,
        },
    },
};

const MAX_U8_ENTRIES: usize = u8::MAX as usize + 1;
const MAX_MULTIPLE_CHOICE_ANSWERS: usize = 4;

/// Create and persist a reusable questions sequence definition on behalf of admins.
pub async fn create_questions_sequence(
    state: &SharedState,
    request: QuestionsSequenceInput,
) -> Result<(QuestionsSequenceSummary, QuestionsSequence), ServiceError> {
    let sequence = build_questions_sequence(request)?;

    // Preserve deterministic ordering based on the assigned question identifiers.
    let question_count = sequence.questions.len() as u32;
    let order: Vec<u32> = (0..question_count).collect();
    let summary: QuestionsSequenceSummary = (sequence.clone(), order).try_into()?;

    let entity: QuestionsSequenceEntity = sequence.clone().into();
    let store = state.require_game_store().await?;
    store.save_questions_sequence(entity).await?;

    Ok((summary, sequence))
}

/// Bootstrap a fresh game during the idle state (with or without a questions sequence).
pub async fn create_game(
    state: &SharedState,
    name: String,
    teams: Vec<TeamInput>,
    questions_sequence_id: Uuid,
    questions_sequence: Option<QuestionsSequence>,
    shuffle_questions: bool,
) -> Result<GameSummary, ServiceError> {
    ensure_idle(state).await?;
    let config = state.config();

    if name.trim().is_empty() {
        return Err(ServiceError::InvalidInput(
            "game name must not be empty".into(),
        ));
    }

    let teams = build_teams(teams, config.as_ref())?;

    let questions_sequence = match questions_sequence {
        Some(sequence) => sequence,
        None => {
            let store = state.require_game_store().await?;
            let sequence_entity = store
                .find_questions_sequence(questions_sequence_id)
                .await?
                .ok_or_else(|| {
                    ServiceError::NotFound(format!(
                        "questions sequence `{questions_sequence_id}` not found"
                    ))
                })?;
            sequence_entity.into()
        }
    };

    if questions_sequence.questions.is_empty() {
        return Err(ServiceError::InvalidInput(
            "questions sequence must contain at least one question".into(),
        ));
    }

    let game = GameSession::new(name, teams, questions_sequence, shuffle_questions);
    if game.question_order.is_empty() {
        return Err(ServiceError::InvalidState(
            "question order is empty after game creation".into(),
        ));
    }

    state
        .with_current_game_slot_mut(|slot| {
            *slot = Some(game.clone());
        })
        .await;

    // Clear all game-scoped state from previous game
    state.clear_game_state().await;

    state.persist_current_game().await?;

    sse_events::broadcast_game_session(state, &game)?;

    Ok(game.try_into()?)
}

/// Load an existing game from the database into the shared state.
pub async fn load_game(
    state: &SharedState,
    id: Uuid,
    shuffle_questions: bool,
) -> Result<GameSummary, ServiceError> {
    ensure_idle(state).await?;

    let store = state.require_game_store().await?;

    let Some(game) = store.find_game(id).await? else {
        return Err(ServiceError::NotFound(format!("game `{id}` not found")));
    };

    if game.question_order.is_empty() {
        return Err(ServiceError::InvalidState(format!(
            "game `{id}` has an empty question order"
        )));
    }

    let current_question_index = game.current_question_index;
    let current_question_revealed = game.current_question_revealed;
    let is_sequence_in_progress = if let Some(current_question_index) = current_question_index {
        if current_question_revealed && current_question_index >= game.question_order.len() - 1 {
            // Sequence was completed in the previous session
            false
        } else if !current_question_revealed && current_question_index == 0 {
            // Sequence has not been started in the previous session
            false
        } else {
            // Sequence is in progress
            true
        }
    } else {
        // Sequence was completed in the previous session
        false
    };
    if shuffle_questions && is_sequence_in_progress {
        return Err(ServiceError::InvalidInput(
            "shuffle parameter cannot be used: game is already in progress".into(),
        ));
    }

    let Some(questions_sequence) = store
        .find_questions_sequence(game.questions_sequence_id)
        .await?
    else {
        return Err(ServiceError::NotFound(format!(
            "questions sequence `{}` not found",
            game.questions_sequence_id
        )));
    };

    if questions_sequence.questions.is_empty() {
        return Err(ServiceError::InvalidInput(
            "questions sequence must contain at least one question".into(),
        ));
    }

    validate_persisted_game(&game, &questions_sequence)?;

    let mut game_session: GameSession = (game, questions_sequence).into();

    if shuffle_questions {
        let mut rng = rng();
        game_session.question_order.shuffle(&mut rng);
        game_session.updated_at = SystemTime::now();
    };

    state
        .with_current_game_slot_mut(|slot| {
            *slot = Some(game_session.clone());
        })
        .await;

    // Clear all game-scoped state from previous game
    state.clear_game_state().await;

    if shuffle_questions {
        state.persist_current_game_without_teams().await?;
    }

    sse_events::broadcast_game_session(state, &game_session)?;

    Ok(game_session.try_into()?)
}

/// Ensure a new or loaded game can be bootstrapped in the current state-machine phase.
async fn ensure_idle(state: &SharedState) -> Result<(), ServiceError> {
    let phase = state.state_machine_phase().await;
    if !matches!(phase, state::state_machine::GamePhase::Idle) {
        return Err(ServiceError::InvalidState(
            "game can only be bootstrapped while idle".into(),
        ));
    }
    Ok(())
}

/// Validate incoming DTO teams, applying defaults and allocating a color from the colors set when
/// none is provided. Ensures buzzer IDs remain unique.
fn build_teams(
    teams: Vec<TeamInput>,
    config: &AppConfig,
) -> Result<IndexMap<Uuid, Team>, ServiceError> {
    let mut seen_ids = HashSet::new();
    let mut used_colors = Vec::new();

    teams
        .into_iter()
        .map(|team| {
            let buzzer_id = team
                .buzzer_id
                .unwrap_or_default()
                .as_ref()
                .map(|id| {
                    if !seen_ids.insert(id.clone()) {
                        Err(ServiceError::InvalidInput(format!(
                            "duplicate buzzer id `{}` detected",
                            id
                        )))
                    } else {
                        Ok(id.clone())
                    }
                })
                .transpose()?;

            if team.name.trim().is_empty() {
                return Err(ServiceError::InvalidInput(
                    "team name must not be empty".into(),
                ));
            }

            // Pick the first free color; fall back to the colors set order if everything is taken.
            let color = team
                .color
                .map(Into::into)
                .unwrap_or_else(|| config.first_unused_color(&used_colors));
            used_colors.push(color.clone());

            let team = Team {
                buzzer_id,
                name: team.name,
                score: team.score.unwrap_or_default(),
                color,
                updated_at: SystemTime::now(),
            };

            Ok((Uuid::new_v4(), team))
        })
        .collect()
}

/// Construct a questions sequence from user-provided question metadata.
fn build_questions_sequence(
    input: QuestionsSequenceInput,
) -> Result<QuestionsSequence, ServiceError> {
    let QuestionsSequenceInput { name, questions } = input;

    if name.trim().is_empty() {
        return Err(ServiceError::InvalidInput(
            "questions sequence name must not be empty".into(),
        ));
    }
    if questions.is_empty() {
        return Err(ServiceError::InvalidInput(
            "questions sequence must not be empty".into(),
        ));
    }

    let questions = questions
        .into_iter()
        .enumerate()
        .map(|(index, question)| {
            let question = build_question(question)?;
            Ok(((index as u32), question))
        })
        .collect::<Result<IndexMap<u32, Question>, ServiceError>>()?;

    Ok(QuestionsSequence::new(name, questions))
}

/// Build a runtime question model from an already validated question input DTO.
fn build_question(question: QuestionInput) -> Result<Question, ServiceError> {
    match question {
        QuestionInput::BlindTest(question) => {
            if question.url.trim().is_empty() {
                return Err(ServiceError::InvalidInput(
                    "blindtest question url must not be empty".into(),
                ));
            }
            ensure_guess_duration(question.guess_duration_ms)?;
            ensure_not_empty(question.answers.len(), "blindtest question answers")?;

            Ok(Question::BlindTest(BlindTestQuestion {
                starts_at_ms: question.starts_at_ms,
                guess_duration_ms: question.guess_duration_ms,
                url: question.url,
                answers: assign_ids(
                    question.answers,
                    "blindtest question answers",
                    build_blindtest_answer,
                )?,
            }))
        }
        QuestionInput::MultipleChoice(question) => {
            ensure_guess_duration(question.guess_duration_ms)?;
            ensure_prompt(&question.prompt)?;
            ensure_not_empty(question.answers.len(), "multiple-choice question answers")?;
            if question.answers.len() > MAX_MULTIPLE_CHOICE_ANSWERS {
                return Err(ServiceError::InvalidInput(format!(
                    "multiple-choice questions cannot declare more than {MAX_MULTIPLE_CHOICE_ANSWERS} answers"
                )));
            }

            Ok(Question::MultipleChoice(MultipleChoiceQuestion {
                guess_duration_ms: question.guess_duration_ms,
                prompt: question.prompt,
                url: empty_string_as_none(question.url),
                answers: assign_ids(
                    question.answers,
                    "multiple-choice question answers",
                    build_multiple_choice_answer,
                )?,
                hints: assign_ids(question.hints, "multiple-choice question hints", build_hint)?,
            }))
        }
        QuestionInput::Open(question) => {
            ensure_guess_duration(question.guess_duration_ms)?;
            ensure_prompt(&question.prompt)?;
            ensure_not_empty(question.answers.len(), "open question answers")?;

            Ok(Question::Open(OpenQuestion {
                guess_duration_ms: question.guess_duration_ms,
                prompt: question.prompt,
                url: empty_string_as_none(question.url),
                answers: assign_ids(question.answers, "open question answers", build_open_answer)?,
                hints: assign_ids(question.hints, "open question hints", build_hint)?,
            }))
        }
    }
}

/// Build a runtime blindtest answer from input while enforcing non-empty text fields.
fn build_blindtest_answer(answer: BlindTestAnswerInput) -> Result<BlindTestAnswer, ServiceError> {
    if answer.key.trim().is_empty() {
        return Err(ServiceError::InvalidInput(
            "blindtest answer key must not be empty".into(),
        ));
    }
    if answer.value.trim().is_empty() {
        return Err(ServiceError::InvalidInput(
            "blindtest answer value must not be empty".into(),
        ));
    }

    Ok(BlindTestAnswer {
        key: answer.key,
        value: answer.value,
        points: answer.points,
        is_bonus: answer.is_bonus,
    })
}

/// Build a runtime multiple-choice answer from input while enforcing non-empty text.
fn build_multiple_choice_answer(
    answer: MultipleChoiceAnswerInput,
) -> Result<MultipleChoiceAnswer, ServiceError> {
    if answer.text.trim().is_empty() {
        return Err(ServiceError::InvalidInput(
            "multiple-choice answer text must not be empty".into(),
        ));
    }

    Ok(MultipleChoiceAnswer {
        text: answer.text,
        is_correct: answer.is_correct,
    })
}

/// Build a runtime open-question answer from input while enforcing non-empty text.
fn build_open_answer(answer: OpenAnswerInput) -> Result<OpenAnswer, ServiceError> {
    if answer.text.trim().is_empty() {
        return Err(ServiceError::InvalidInput(
            "open answer text must not be empty".into(),
        ));
    }

    Ok(OpenAnswer { text: answer.text })
}

/// Build a runtime hint from input while enforcing non-empty text.
fn build_hint(text: String) -> Result<Hint, ServiceError> {
    if text.trim().is_empty() {
        return Err(ServiceError::InvalidInput(
            "question hint text must not be empty".into(),
        ));
    }
    Ok(Hint(text))
}

/// Assign zero-based `u8` IDs to imported values after converting each value.
fn assign_ids<T, U>(
    values: Vec<T>,
    label: &str,
    mut build: impl FnMut(T) -> Result<U, ServiceError>,
) -> Result<HashMap<u8, U>, ServiceError> {
    if values.len() > MAX_U8_ENTRIES {
        return Err(ServiceError::InvalidInput(format!(
            "{label} cannot declare more than {MAX_U8_ENTRIES} entries"
        )));
    }

    values
        .into_iter()
        .enumerate()
        .map(|(index, value)| Ok((index as u8, build(value)?)))
        .collect()
}

/// Ensure a question guess duration is strictly positive.
fn ensure_guess_duration(guess_duration_ms: usize) -> Result<(), ServiceError> {
    if guess_duration_ms == 0 {
        return Err(ServiceError::InvalidInput(
            "guess duration must be strictly positive".into(),
        ));
    }
    Ok(())
}

/// Ensure a text question prompt contains non-whitespace characters.
fn ensure_prompt(prompt: &str) -> Result<(), ServiceError> {
    if prompt.trim().is_empty() {
        return Err(ServiceError::InvalidInput(
            "question prompt must not be empty".into(),
        ));
    }
    Ok(())
}

/// Ensure an imported collection contains at least one element.
fn ensure_not_empty(count: usize, label: &str) -> Result<(), ServiceError> {
    if count == 0 {
        return Err(ServiceError::InvalidInput(format!(
            "{label} must not be empty"
        )));
    }
    Ok(())
}

/// Normalize optional string fields by treating blank strings as absent values.
fn empty_string_as_none(value: Option<String>) -> Option<String> {
    value.and_then(|url| {
        if url.trim().is_empty() {
            None
        } else {
            Some(url)
        }
    })
}

/// Validate persisted game references before rebuilding an in-memory game session.
fn validate_persisted_game(
    game: &GameEntity,
    questions_sequence: &QuestionsSequenceEntity,
) -> Result<(), ServiceError> {
    if game.teams.is_empty() {
        return Err(ServiceError::InvalidState(format!(
            "game `{}` has no registered teams",
            game.id
        )));
    }

    if questions_sequence.questions.is_empty() {
        return Err(ServiceError::InvalidState(format!(
            "game `{}` has an empty questions sequence",
            game.id
        )));
    }

    let sequence_questions_nb = questions_sequence.questions.len();
    let question_order = &game.question_order;
    if question_order.len() != sequence_questions_nb {
        return Err(ServiceError::InvalidState(format!(
            "game `{}` question order is inconsistent (expected {} entries, got {})",
            game.id,
            sequence_questions_nb,
            question_order.len()
        )));
    }

    let question_ids = (0..sequence_questions_nb as u32).collect::<HashSet<_>>();
    for question_id in question_order {
        if !question_ids.contains(question_id) {
            return Err(ServiceError::InvalidState(format!(
                "game `{}` question order references unknown question `{}`",
                game.id, question_id
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::game::{
        BlindTestAnswerInput, BlindTestQuestionInput, MultipleChoiceAnswerInput,
        MultipleChoiceQuestionInput, OpenAnswerInput, OpenQuestionInput,
    };
    use validator::Validate;

    fn blindtest_question() -> QuestionInput {
        QuestionInput::BlindTest(BlindTestQuestionInput {
            starts_at_ms: 1_000,
            guess_duration_ms: 15_000,
            url: "https://example.com/song.mp3".to_string(),
            answers: vec![
                BlindTestAnswerInput {
                    key: "title".to_string(),
                    value: "Song title".to_string(),
                    points: 2,
                    is_bonus: false,
                },
                BlindTestAnswerInput {
                    key: "year".to_string(),
                    value: "1999".to_string(),
                    points: 1,
                    is_bonus: true,
                },
            ],
        })
    }

    #[test]
    fn imports_all_question_variants_with_auto_ids() {
        let input = QuestionsSequenceInput {
            name: "Quiz".to_string(),
            questions: vec![
                blindtest_question(),
                QuestionInput::MultipleChoice(MultipleChoiceQuestionInput {
                    guess_duration_ms: 10_000,
                    prompt: "Pick one".to_string(),
                    url: None,
                    answers: vec![
                        MultipleChoiceAnswerInput {
                            text: "A".to_string(),
                            is_correct: true,
                        },
                        MultipleChoiceAnswerInput {
                            text: "B".to_string(),
                            is_correct: false,
                        },
                    ],
                    hints: vec!["first hint".to_string()],
                }),
                QuestionInput::Open(OpenQuestionInput {
                    guess_duration_ms: 10_000,
                    prompt: "Write it".to_string(),
                    url: Some("https://example.com/prompt.png".to_string()),
                    answers: vec![OpenAnswerInput {
                        text: "accepted".to_string(),
                    }],
                    hints: vec!["open hint".to_string()],
                }),
            ],
        };

        input.validate().unwrap();
        let sequence = build_questions_sequence(input).unwrap();

        assert_eq!(sequence.questions.len(), 3);
        match sequence.questions.get(&0).unwrap() {
            Question::BlindTest(question) => {
                assert!(question.answers.contains_key(&0));
                assert!(question.answers.contains_key(&1));
                assert!(question.answers.get(&1).unwrap().is_bonus);
            }
            other => panic!("expected blindtest question, got {other:?}"),
        }
        match sequence.questions.get(&1).unwrap() {
            Question::MultipleChoice(question) => {
                assert!(question.answers.get(&0).unwrap().is_correct);
                assert!(!question.answers.get(&1).unwrap().is_correct);
                assert_eq!(question.hints.get(&0).unwrap().0, "first hint");
            }
            other => panic!("expected multiple-choice question, got {other:?}"),
        }
        match sequence.questions.get(&2).unwrap() {
            Question::Open(question) => {
                assert_eq!(question.answers.get(&0).unwrap().text, "accepted");
                assert_eq!(question.hints.get(&0).unwrap().0, "open hint");
            }
            other => panic!("expected open question, got {other:?}"),
        }
    }

    #[test]
    fn rejects_empty_sequence() {
        let input = QuestionsSequenceInput {
            name: "Quiz".to_string(),
            questions: Vec::new(),
        };

        input.validate().unwrap();
        let err = build_questions_sequence(input).unwrap_err();

        assert!(err.to_string().contains("questions sequence"));
    }

    #[test]
    fn rejects_multiple_choice_with_more_than_four_answers() {
        let input = QuestionsSequenceInput {
            name: "Quiz".to_string(),
            questions: vec![QuestionInput::MultipleChoice(MultipleChoiceQuestionInput {
                guess_duration_ms: 10_000,
                prompt: "Pick one".to_string(),
                url: None,
                answers: (0..5)
                    .map(|index| MultipleChoiceAnswerInput {
                        text: format!("answer {index}"),
                        is_correct: index == 0,
                    })
                    .collect(),
                hints: Vec::new(),
            })],
        };

        input.validate().unwrap();
        let err = build_questions_sequence(input).unwrap_err();

        assert!(err.to_string().contains("more than 4 answers"));
    }

    #[test]
    fn rejects_invalid_url() {
        let input = QuestionsSequenceInput {
            name: "Quiz".to_string(),
            questions: vec![QuestionInput::BlindTest(BlindTestQuestionInput {
                starts_at_ms: 0,
                guess_duration_ms: 10_000,
                url: "not a url".to_string(),
                answers: vec![BlindTestAnswerInput {
                    key: "title".to_string(),
                    value: "Song".to_string(),
                    points: 1,
                    is_bonus: false,
                }],
            })],
        };

        let err = input.validate().unwrap_err();
        assert!(err.to_string().contains("url"));
    }

    #[test]
    #[allow(deprecated)]
    fn converts_legacy_playlist_to_blindtest_questions() {
        let input = crate::dto::game::LegacyPlaylistInput {
            name: "Legacy".to_string(),
            songs: vec![crate::dto::game::LegacySongInput {
                starts_at_ms: 500,
                guess_duration_ms: 9_000,
                url: "https://example.com/legacy.mp3".to_string(),
                point_fields: vec![crate::dto::game::LegacyPointFieldInput {
                    key: "artist".to_string(),
                    value: "Artist".to_string(),
                    points: 3,
                }],
                bonus_fields: vec![crate::dto::game::LegacyPointFieldInput {
                    key: "year".to_string(),
                    value: "2001".to_string(),
                    points: 1,
                }],
            }],
        };

        let converted: QuestionsSequenceInput = input.into();
        converted.validate().unwrap();
        let sequence = build_questions_sequence(converted).unwrap();
        match sequence.questions.get(&0).unwrap() {
            Question::BlindTest(question) => {
                assert!(!question.answers.get(&0).unwrap().is_bonus);
                assert!(question.answers.get(&1).unwrap().is_bonus);
            }
            other => panic!("expected blindtest question, got {other:?}"),
        }
    }
}
