use std::{collections::HashMap, time::SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::dao::{
    game_store::couchdb::error::CouchDaoError,
    models::{GameEntity, QuestionEntity, QuestionsSequenceEntity, TeamColorEntity, TeamEntity},
};

pub const GAME_PREFIX: &str = "game::";
pub const QUESTIONS_SEQUENCE_PREFIX: &str = "questions_sequence::";
pub const TEAM_PREFIX: &str = "team::";
pub const END_SUFFIX: &str = "\u{ffff}";

#[derive(Debug, Deserialize)]
pub struct AllDocsResponse {
    pub rows: Vec<AllDocsRow>,
}

#[derive(Debug, Deserialize)]
pub struct AllDocsRow {
    #[serde(default)]
    pub doc: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouchGameDocument {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_rev", skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    #[serde(flatten)]
    pub game: GameBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameBody {
    pub name: String,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
    pub team_ids: Vec<Uuid>, // List of team IDs in their display order
    pub questions_sequence_id: Uuid,
    pub question_order: Vec<u32>,
    pub current_question_index: Option<usize>,
    pub current_question_revealed: bool,
}

impl From<(GameEntity, Option<String>)> for CouchGameDocument {
    fn from((game, rev): (GameEntity, Option<String>)) -> Self {
        let team_ids: Vec<Uuid> = game.teams.iter().map(|t| t.id).collect();
        Self {
            id: game_doc_id(game.id),
            rev,
            game: GameBody {
                name: game.name,
                created_at: game.created_at,
                updated_at: game.updated_at,
                team_ids,
                questions_sequence_id: game.questions_sequence_id,
                question_order: game.question_order,
                current_question_index: game.current_question_index,
                current_question_revealed: game.current_question_revealed,
            },
        }
    }
}

impl CouchGameDocument {
    pub fn try_into_entity(
        self,
        id: Uuid,
        team_docs: Vec<CouchTeamDocument>,
    ) -> Result<GameEntity, CouchDaoError> {
        // First compute the latest update timestamp as max of game and all team updates
        let updated_at = team_docs
            .iter()
            .map(|doc| doc.team.updated_at)
            .chain(std::iter::once(self.game.updated_at))
            .max()
            .unwrap_or(self.game.updated_at);

        // Convert team documents into a map for easy lookup
        let mut team_map = team_docs
            .into_iter()
            .map(|team_doc| {
                let team_entity: TeamEntity = team_doc.into();
                (team_entity.id, team_entity)
            })
            .collect::<HashMap<_, _>>();

        // Find any missing team IDs
        let missing_team_ids = self
            .game
            .team_ids
            .iter()
            .filter(|id| !team_map.contains_key(id))
            .copied()
            .collect::<Vec<_>>();

        if !missing_team_ids.is_empty() {
            return Err(CouchDaoError::MissingTeams {
                game_id: self.id.clone(),
                team_ids: missing_team_ids,
            });
        }

        // Create teams vector in the order specified by game_doc.team_ids
        // Use remove to take ownership without cloning
        let teams = self
            .game
            .team_ids
            .iter()
            .map(|id| {
                team_map
                    .remove(id)
                    .ok_or_else(|| CouchDaoError::MissingTeams {
                        game_id: self.id.clone(),
                        team_ids: vec![*id],
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Create game entity with teams
        Ok(GameEntity {
            id,
            name: self.game.name,
            created_at: self.game.created_at,
            updated_at, // Use computed max timestamp
            teams,
            questions_sequence_id: self.game.questions_sequence_id,
            question_order: self.game.question_order,
            current_question_index: self.game.current_question_index,
            current_question_revealed: self.game.current_question_revealed,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouchQuestionsSequenceDocument {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_rev", skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    #[serde(flatten)]
    pub questions_sequence: QuestionsSequenceBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionsSequenceBody {
    pub name: String,
    pub questions: Vec<QuestionEntity>,
}

impl From<(QuestionsSequenceEntity, Option<String>)> for CouchQuestionsSequenceDocument {
    fn from((value, rev): (QuestionsSequenceEntity, Option<String>)) -> Self {
        Self {
            id: questions_sequence_doc_id(value.id),
            rev,
            questions_sequence: QuestionsSequenceBody {
                name: value.name,
                questions: value.questions,
            },
        }
    }
}

impl TryFrom<CouchQuestionsSequenceDocument> for QuestionsSequenceEntity {
    type Error = CouchDaoError;

    fn try_from(doc: CouchQuestionsSequenceDocument) -> Result<Self, Self::Error> {
        Ok(Self {
            id: extract_uuid(&doc.id)?,
            name: doc.questions_sequence.name,
            questions: doc.questions_sequence.questions,
        })
    }
}

pub fn game_doc_id(id: Uuid) -> String {
    format!("{}{}", GAME_PREFIX, id)
}

pub fn questions_sequence_doc_id(id: Uuid) -> String {
    format!("{}{}", QUESTIONS_SEQUENCE_PREFIX, id)
}

pub fn extract_uuid(doc_id: &str) -> Result<Uuid, CouchDaoError> {
    let (_, id) = doc_id
        .split_once("::")
        .ok_or_else(|| CouchDaoError::InvalidDocId {
            doc_id: doc_id.to_string(),
            kind: "missing separator",
        })?;

    Uuid::parse_str(id).map_err(|_| CouchDaoError::InvalidDocId {
        doc_id: doc_id.to_string(),
        kind: "invalid UUID",
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouchTeamDocument {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_rev", skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    #[serde(flatten)]
    pub team: TeamBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamBody {
    pub game_id: Uuid,
    pub team_id: Uuid,
    pub name: String,
    pub score: i32,
    pub color: TeamColorEntity,
    pub updated_at: SystemTime,
}

impl From<(Uuid, TeamEntity, Option<String>)> for CouchTeamDocument {
    fn from((game_id, team, rev): (Uuid, TeamEntity, Option<String>)) -> Self {
        Self {
            id: team_doc_id(game_id, team.id),
            rev,
            team: TeamBody {
                game_id,
                team_id: team.id,
                name: team.name,
                score: team.score,
                color: team.color,
                updated_at: team.updated_at,
            },
        }
    }
}

impl From<CouchTeamDocument> for TeamEntity {
    fn from(doc: CouchTeamDocument) -> Self {
        TeamEntity {
            id: doc.team.team_id,
            name: doc.team.name,
            score: doc.team.score,
            color: doc.team.color,
            updated_at: doc.team.updated_at,
        }
    }
}

pub fn team_doc_id(game_id: Uuid, team_id: Uuid) -> String {
    format!("{}{}:{}", TEAM_PREFIX, game_id, team_id)
}
