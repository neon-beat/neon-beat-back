use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Minimal persisted game state snapshot.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct GameState {
    pub quiz_name: String,
    pub round: u32,
}
