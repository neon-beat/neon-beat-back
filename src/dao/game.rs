use crate::dao::models::{GameEntity, PlaylistEntity};
use crate::dao::storage::StorageResult;
use futures::future::BoxFuture;
use uuid::Uuid;

/// Abstraction over the persistence layer for game sessions and playlists.
pub trait GameStore: Send + Sync {
    fn save_game(&self, game: GameEntity) -> BoxFuture<'static, StorageResult<()>>;
    fn save_playlist(&self, playlist: PlaylistEntity) -> BoxFuture<'static, StorageResult<()>>;
    fn find_game(&self, id: Uuid) -> BoxFuture<'static, StorageResult<Option<GameEntity>>>;
    fn find_playlist(&self, id: Uuid) -> BoxFuture<'static, StorageResult<Option<PlaylistEntity>>>;
    fn list_games(&self) -> BoxFuture<'static, StorageResult<Vec<(Uuid, String)>>>;
    fn list_playlists(&self) -> BoxFuture<'static, StorageResult<Vec<(Uuid, String)>>>;
    fn health_check(&self) -> BoxFuture<'static, StorageResult<()>>;
}
