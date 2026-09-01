pub mod auth;
pub mod db;
pub mod router;
pub mod session;

use sqlx::SqlitePool;

pub use crate::router::router;

/// Shared state for all request handlers.
pub struct AppState {
    pub pool: SqlitePool,
    pub sessions: session::SessionStore,
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            sessions: session::SessionStore::default(),
        }
    }
}
