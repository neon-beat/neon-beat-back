use mongodb::bson::{Binary, DateTime, Document, doc, spec::BinarySubtype};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// MongoDB document models used by the MongoGameStore.
//
// Indexes created in `ensure_indexes()`:
// - games collection:
//   - `game_name_idx` on { name: 1 } (non-unique) — used to search/list games by name.
// - teams collection:
//   - `team_game_idx` on { game_id: 1, team_id: 1 } (unique) — enforces one team_id per game
//     and enables efficient lookup of a team's document within a game.
use crate::dao::models::{GameEntity, TeamColorEntity, TeamEntity};

/// Representation of a game document stored in MongoDB.
///
/// Indexes:
/// - `_id` (implicit) — unique identifier for the document.
/// - `name` — indexed as `game_name_idx` (non-unique) for fast lookup/listing by name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MongoGameDocument {
    /// Document `_id` (UUID) — unique primary key.
    #[serde(rename = "_id")]
    id: Uuid,
    /// Game display name. Indexed (non-unique) as `game_name_idx`.
    name: String,
    /// Creation timestamp stored as BSON DateTime.
    created_at: DateTime,
    /// Last update timestamp stored as BSON DateTime.
    updated_at: DateTime,
    /// List of team ids in display order. Individual team details live in the `teams`
    /// collection as `MongoTeamDocument` documents.
    pub teams: Vec<Uuid>,
    /// Referenced questions sequence id.
    questions_sequence_id: Uuid,
    /// Ordered list of question indices referencing the sequence.
    question_order: Vec<u32>,
    /// Optional index of current question.
    current_question_index: Option<usize>,
    /// Whether the current question has been revealed. Default false.
    current_question_revealed: bool,
}

impl From<GameEntity> for MongoGameDocument {
    fn from(game: GameEntity) -> Self {
        let team_ids: Vec<Uuid> = game.teams.iter().map(|t| t.id).collect();
        Self {
            id: game.id,
            name: game.name,
            created_at: DateTime::from_system_time(game.created_at),
            updated_at: DateTime::from_system_time(game.updated_at),
            teams: team_ids,
            questions_sequence_id: game.questions_sequence_id,
            question_order: game.question_order,
            current_question_index: game.current_question_index,
            current_question_revealed: game.current_question_revealed,
        }
    }
}

impl From<MongoGameDocument> for GameEntity {
    fn from(value: MongoGameDocument) -> Self {
        Self {
            id: value.id,
            name: value.name,
            created_at: value.created_at.to_system_time(),
            updated_at: value.updated_at.to_system_time(),
            // Teams must be loaded from the `teams` collection; using empty vector here
            // is a placeholder for call sites that should fetch team documents.
            teams: Vec::new(),
            questions_sequence_id: value.questions_sequence_id,
            question_order: value.question_order,
            current_question_index: value.current_question_index,
            current_question_revealed: value.current_question_revealed,
        }
    }
}

pub fn uuid_as_binary(id: Uuid) -> Binary {
    Binary {
        subtype: BinarySubtype::Uuid,
        bytes: id.into_bytes().to_vec(),
    }
}

pub fn doc_id(id: Uuid) -> Document {
    doc! {"_id": uuid_as_binary(id)}
}

/// Standalone team document stored in the `teams` collection (storage-only).
///
/// Indexes:
/// - Compound `{ game_id: 1, team_id: 1 }` — created as `team_game_idx` and is unique to
///   guarantee a single team document per (game, team) pair and to support quick lookups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MongoTeamDocument {
    /// Owning game UUID. Indexed as part of the compound `team_game_idx`.
    pub game_id: Uuid,
    /// Team UUID within the game. Indexed as part of the compound `team_game_idx`.
    pub team_id: Uuid,
    /// Team display name.
    pub name: String,
    /// Team score.
    pub score: i32,
    /// Team color.
    pub color: TeamColorEntity,
    /// Last update timestamp stored as BSON DateTime.
    pub updated_at: DateTime,
}

impl From<(Uuid, TeamEntity)> for MongoTeamDocument {
    fn from((game_id, team): (Uuid, TeamEntity)) -> Self {
        Self {
            game_id,
            team_id: team.id,
            name: team.name,
            score: team.score,
            color: team.color,
            updated_at: DateTime::from_system_time(team.updated_at),
        }
    }
}

impl From<MongoTeamDocument> for (Uuid, TeamEntity) {
    fn from(doc: MongoTeamDocument) -> Self {
        let team = TeamEntity {
            id: doc.team_id,
            name: doc.name,
            score: doc.score,
            color: doc.color,
            updated_at: doc.updated_at.to_system_time(),
        };
        (doc.team_id, team)
    }
}
