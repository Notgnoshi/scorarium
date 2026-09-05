use std::sync::Arc;

use axum_test::TestServer;
use scorarium::{AppState, auth, db, demo};

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

    pub fn library(mut self, name: &str) -> Self {
        self.libraries.push(name.to_string());
        self
    }

    /// Claim the login password, as if a first login already happened.
    pub fn password(mut self, password: &str) -> Self {
        self.password = Some(password.to_string());
        self
    }

    pub async fn build(self) -> AppState {
        let pool = db::connect_in_memory()
            .await
            .expect("failed to open in-memory database");
        if self.demo {
            demo::populate(&pool)
                .await
                .expect("failed to populate demo data");
        }

        for name in &self.libraries {
            db::create_library(&pool, name)
                .await
                .expect("failed to create test library");
        }
        if let Some(password) = &self.password {
            auth::claim_password(&pool, password)
                .await
                .expect("failed to claim test password");
        }
        if self.demo {
            return AppState::demo(pool);
        }
        // The test server speaks plain HTTP, so mirror a development run
        AppState::new(pool, false)
    }
}

/// A test server that, like a browser, saves cookies across requests.
pub fn browser(state: AppState) -> TestServer {
    let mut server = TestServer::new(scorarium::router(Arc::new(state)));
    server.save_cookies();
    server
}
