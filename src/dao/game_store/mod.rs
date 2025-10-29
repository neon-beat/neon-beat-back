/// CouchDB game store implementation.
#[cfg(feature = "couch-store")]
pub mod couchdb;
/// MongoDB game store implementation.
#[cfg(feature = "mongo-store")]
pub mod mongodb;

use crate::dao::models::{GameEntity, GameListItemEntity, PlaylistEntity, TeamEntity};
use crate::dao::storage::StorageResult;
use futures::future::BoxFuture;
use uuid::Uuid;

/// Abstraction over the persistence layer for game sessions and playlists.
pub trait GameStore: Send + Sync {
    /// Save a complete game entity including all team documents.
    fn save_game(&self, game: GameEntity) -> BoxFuture<'static, StorageResult<()>>;
    /// Save only the game document without updating team documents.
    fn save_game_without_teams(&self, game: GameEntity) -> BoxFuture<'static, StorageResult<()>>;
    /// Save a playlist entity to storage.
    fn save_playlist(&self, playlist: PlaylistEntity) -> BoxFuture<'static, StorageResult<()>>;
    /// Find and retrieve a game entity by ID.
    fn find_game(&self, id: Uuid) -> BoxFuture<'static, StorageResult<Option<GameEntity>>>;
    /// Find and retrieve a playlist entity by ID.
    fn find_playlist(&self, id: Uuid) -> BoxFuture<'static, StorageResult<Option<PlaylistEntity>>>;
    /// List all game entities with summary information.
    fn list_games(&self) -> BoxFuture<'static, StorageResult<Vec<GameListItemEntity>>>;
    /// List all playlists with ID and name pairs.
    fn list_playlists(&self) -> BoxFuture<'static, StorageResult<Vec<(Uuid, String)>>>;
    /// Delete a game entity and all its associated team documents.
    fn delete_game(&self, id: Uuid) -> BoxFuture<'static, StorageResult<bool>>;
    /// Save a single team document for a game.
    fn save_team(&self, game_id: Uuid, team: TeamEntity) -> BoxFuture<'static, StorageResult<()>>;
    /// Delete a single team document from a game.
    fn delete_team(&self, game_id: Uuid, team_id: Uuid) -> BoxFuture<'static, StorageResult<()>>;
    /// Verify storage backend is reachable and operational.
    fn health_check(&self) -> BoxFuture<'static, StorageResult<()>>;
    /// Attempt to reconnect to the storage backend after a disconnection.
    fn try_reconnect(&self) -> BoxFuture<'static, StorageResult<()>>;
}
