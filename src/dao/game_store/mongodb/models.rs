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
    /// Referenced playlist id.
    playlist_id: Uuid,
    /// Ordered list of song indices referencing the playlist.
    playlist_song_order: Vec<u32>,
    /// Optional index of current song.
    current_song_index: Option<usize>,
    /// Whether the current song has been found. Default false.
    current_song_found: bool,
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
            playlist_id: game.playlist_id,
            playlist_song_order: game.playlist_song_order,
            current_song_index: game.current_song_index,
            current_song_found: game.current_song_found,
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
            playlist_id: value.playlist_id,
            playlist_song_order: value.playlist_song_order,
            current_song_index: value.current_song_index,
            current_song_found: value.current_song_found,
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

impl TryFrom<MongoTeamDocument> for (Uuid, TeamEntity) {
    type Error = mongodb::error::Error;

    fn try_from(doc: MongoTeamDocument) -> Result<Self, Self::Error> {
        let team = TeamEntity {
            id: doc.team_id,
            name: doc.name,
            score: doc.score,
            color: doc.color,
            updated_at: doc.updated_at.to_system_time(),
        };
        Ok((doc.team_id, team))
    }
}
