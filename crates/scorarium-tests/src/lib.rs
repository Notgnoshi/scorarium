use scorarium::{AppState, db};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

/// Builds an [AppState] backed by a fresh in-memory database with the given contents.
#[derive(Default)]
pub struct TestDb {
    libraries: Vec<String>,
}

impl TestDb {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn library(mut self, name: &str) -> Self {
        self.libraries.push(name.to_string());
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
        AppState { pool }
    }
}
