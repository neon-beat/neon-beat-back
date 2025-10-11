pub mod error;
pub mod game;
pub mod manager;

pub use error::MongoDaoError;
pub use game::MongoGameStore;
pub use manager::{MongoManager, connect, ensure_indexes};
