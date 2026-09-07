pub mod db;
pub mod import;
pub mod publication_form;
pub mod router;
pub mod session;
pub mod work_form;

use scorarium_archive::Archive;
// TODO: Remove once identifier parsing is no longer needed by the web crate directly
pub use scorarium_archive::identifier;
use sqlx::SqlitePool;

pub use crate::router::router;

/// Shared state for all request handlers.
pub struct AppState {
    pub archive: Archive,
    // TODO: Remove the pool once all route migrate over to the new Archive.
    pub pool: SqlitePool,
    pub sessions: session::SessionStore,
    pub drafts: import::DraftStore,
    /// Whether the login cookie is marked Secure
    pub secure_cookies: bool,
    pub demo: bool,
}

impl AppState {
    pub fn new(archive: Archive, secure_cookies: bool) -> Self {
        Self {
            pool: archive.pool().clone(),
            archive,
            sessions: session::SessionStore::default(),
            drafts: import::DraftStore::default(),
            secure_cookies,
            demo: false,
        }
    }

    pub fn demo(archive: Archive) -> Self {
        Self {
            demo: true,
            ..Self::new(archive, true)
        }
    }
}
