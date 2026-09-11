use std::sync::Arc;

use axum_test::{TestResponse, TestServer};
use scorarium::AppState;
use scorarium_archive::Archive;

/// Builds an [AppState] backed by a fresh in-memory database with the given contents.
#[derive(Default)]
pub struct TestDb {
    demo: bool,
    libraries: Vec<String>,
    password: Option<String>,
}

impl TestDb {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start from the demo libraries that `--demo` serves.
    pub fn demo(mut self) -> Self {
        self.demo = true;
        self
    }

    /// A public library, since most web tests read pages without logging in
    pub fn library(mut self, name: &str) -> Self {
        self.libraries.push(name.to_string());
        self
    }

    /// Claim the login password, as if a first login already happened.
    pub fn password(mut self, password: &str) -> Self {
        self.password = Some(password.to_string());
        self
    }

    pub async fn build(self) -> Arc<AppState> {
        let archive = Archive::in_memory()
            .await
            .expect("failed to open in-memory archive");
        if self.demo {
            archive
                .populate_demo()
                .await
                .expect("failed to populate demo data");
        }

        for name in &self.libraries {
            archive
                .create_library(name, false)
                .await
                .expect("failed to create test library");
        }
        if let Some(password) = &self.password {
            archive
                .claim_password(password)
                .await
                .expect("failed to claim test password");
        }
        if self.demo {
            return Arc::new(AppState::demo(archive));
        }
        // The test server speaks plain HTTP, so mirror a development run
        Arc::new(AppState::new(archive, false))
    }
}

/// A test server that, like a browser, saves cookies across requests.
///
/// The state is shared rather than handed over, so a test can keep looking things up in the
/// archive while the server serves from it.
pub fn browser(state: Arc<AppState>) -> TestServer {
    let mut server = TestServer::new(scorarium::router(state));
    server.save_cookies();
    server
}

/// Submit the demo mode's login form, which has no fields.
///
/// A browser still sends an empty form with the form content type, which the login handler
/// requires even though the demo ignores the form.
pub async fn demo_login(server: &TestServer) -> TestResponse {
    server
        .post("/login")
        .content_type("application/x-www-form-urlencoded")
        .await
}
