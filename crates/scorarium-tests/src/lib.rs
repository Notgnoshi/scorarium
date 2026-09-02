use std::sync::Arc;

use axum_test::TestServer;
use scorarium::{AppState, auth, db};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// Builds an [AppState] backed by a fresh in-memory database with the given contents.
#[derive(Default)]
pub struct TestDb {
    libraries: Vec<String>,
    password: Option<String>,
}

impl TestDb {
    pub fn new() -> Self {
        Self::default()
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
        let options = SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true);
        // Each connection to an in-memory SQLite database is its own separate database, so pin the
        // pool to a single connection that is never recycled.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await
            .expect("failed to open in-memory database");
        db::MIGRATOR
            .run(&pool)
            .await
            .expect("failed to migrate test database");

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
        AppState::new(pool)
    }
}

/// A test server that, like a browser, saves cookies across requests.
pub fn browser(state: AppState) -> TestServer {
    let mut server = TestServer::new(scorarium::router(Arc::new(state)));
    server.save_cookies();
    server
}
