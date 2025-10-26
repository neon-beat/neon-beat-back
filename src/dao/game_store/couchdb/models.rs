use std::{collections::HashMap, time::SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::dao::{
    game_store::couchdb::error::CouchDaoError,
    models::{GameEntity, PlaylistEntity, SongEntity, TeamColorEntity, TeamEntity},
};

pub const GAME_PREFIX: &str = "game::";
pub const PLAYLIST_PREFIX: &str = "playlist::";
pub const TEAM_PREFIX: &str = "team::";
pub const END_SUFFIX: &str = "\u{ffff}";

#[derive(Debug, Deserialize)]
pub struct AllDocsResponse {
    pub rows: Vec<AllDocsRow>,
}

#[derive(Debug, Deserialize)]
pub struct AllDocsRow {
    pub id: String,
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
    pub playlist_id: Uuid,
    pub playlist_song_order: Vec<u32>,
    pub current_song_index: Option<usize>,
    pub current_song_found: bool,
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
                playlist_id: game.playlist_id,
                playlist_song_order: game.playlist_song_order,
                current_song_index: game.current_song_index,
                current_song_found: game.current_song_found,
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
            playlist_id: self.game.playlist_id,
            playlist_song_order: self.game.playlist_song_order,
            current_song_index: self.game.current_song_index,
            current_song_found: self.game.current_song_found,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CouchPlaylistDocument {
    #[serde(rename = "_id")]
    pub id: String,
    #[serde(rename = "_rev", skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    #[serde(flatten)]
    pub playlist: PlaylistBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistBody {
    pub name: String,
    pub songs: Vec<SongEntity>,
}

impl From<(PlaylistEntity, Option<String>)> for CouchPlaylistDocument {
    fn from((value, rev): (PlaylistEntity, Option<String>)) -> Self {
        Self {
            id: playlist_doc_id(value.id),
            rev,
            playlist: PlaylistBody {
                name: value.name,
                songs: value.songs,
            },
        }
    }
}

impl TryFrom<CouchPlaylistDocument> for PlaylistEntity {
    type Error = CouchDaoError;

    fn try_from(doc: CouchPlaylistDocument) -> Result<Self, Self::Error> {
        Ok(Self {
            id: extract_uuid(&doc.id)?,
            name: doc.playlist.name,
            songs: doc.playlist.songs,
        })
    }
}

pub fn game_doc_id(id: Uuid) -> String {
    format!("{}{}", GAME_PREFIX, id)
}

pub fn playlist_doc_id(id: Uuid) -> String {
    format!("{}{}", PLAYLIST_PREFIX, id)
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
    pub buzzer_id: Option<String>,
    pub color: TeamColorEntity,
    pub updated_at: SystemTime,
}

impl From<(Uuid, TeamEntity, Option<String>)> for CouchTeamDocument {
    fn from((game_id, team, rev): (Uuid, TeamEntity, Option<String>)) -> Self {
        Self {
            id: team_doc_id(game_id, team.id),
            rev,
            team: TeamBody {
                game_id: game_id,
                team_id: team.id,
                name: team.name,
                score: team.score,
                buzzer_id: team.buzzer_id,
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
            buzzer_id: doc.team.buzzer_id,
            color: doc.team.color,
            updated_at: doc.team.updated_at,
        }
    }
}

pub fn team_doc_id(game_id: Uuid, team_id: Uuid) -> String {
    format!("{}{}:{}", TEAM_PREFIX, game_id, team_id)
}
