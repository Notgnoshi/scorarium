pub mod links;
pub mod publication_post;
pub mod router;
mod serve;
pub mod session;

use scorarium_engine::archive::Archive;
use scorarium_engine::client::Client;

pub use crate::router::router;
pub use crate::serve::{ServeArgs, serve};

/// Shared state for all request handlers.
pub struct AppState {
    pub archive: Archive,
    pub sources: Client,
    pub sessions: session::SessionStore,
    pub throttle: session::LoginThrottle,
    /// Whether the login cookie is marked Secure
    pub secure_cookies: bool,
    pub demo: bool,
}

impl AppState {
    pub fn new(archive: Archive, sources: Client, secure_cookies: bool) -> Self {
        Self {
            archive,
            sources,
            sessions: session::SessionStore::default(),
            throttle: session::LoginThrottle::default(),
            secure_cookies,
            demo: false,
        }
    }

    pub fn demo(archive: Archive, sources: Client) -> Self {
        Self {
            demo: true,
            ..Self::new(archive, sources, true)
        }
    }
}
