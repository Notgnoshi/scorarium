pub mod publication_post;
pub mod router;
pub mod session;

use scorarium_archive::Archive;

pub use crate::router::router;

/// Shared state for all request handlers.
pub struct AppState {
    pub archive: Archive,
    pub sessions: session::SessionStore,
    /// Whether the login cookie is marked Secure
    pub secure_cookies: bool,
    pub demo: bool,
}

impl AppState {
    pub fn new(archive: Archive, secure_cookies: bool) -> Self {
        Self {
            archive,
            sessions: session::SessionStore::default(),
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
