pub mod person;
pub mod publication;
pub mod work;

use std::path::Path;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

/// Open the database in the given data directory, creating and migrating it as necessary.
pub async fn connect(data_dir: &Path) -> color_eyre::Result<SqlitePool> {
    std::fs::create_dir_all(data_dir)?;
    let options = SqliteConnectOptions::new()
        .filename(data_dir.join("scorarium.db"))
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePool::connect_with(options).await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// Open a fresh, migrated in-memory database
pub async fn connect_in_memory() -> sqlx::Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .in_memory(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(options)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// A named container of publications.
#[derive(Debug)]
pub struct Library {
    pub id: i64,
    pub name: String,
}

/// Create a new library, returning its id.
pub async fn create_library(pool: &SqlitePool, name: &str) -> sqlx::Result<i64> {
    let result = sqlx::query!("INSERT INTO library (name) VALUES (?)", name)
        .execute(pool)
        .await?;
    Ok(result.last_insert_rowid())
}

/// Look up a library by id.
pub async fn get_library(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<Library>> {
    sqlx::query_as!(Library, "SELECT id, name FROM library WHERE id = ?", id)
        .fetch_optional(pool)
        .await
}

/// Rename a library. Returns false when no library has that id.
pub async fn rename_library(pool: &SqlitePool, id: i64, name: &str) -> sqlx::Result<bool> {
    let result = sqlx::query!("UPDATE library SET name = ? WHERE id = ?", name, id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Delete a library. Returns false when no library has that id.
pub async fn delete_library(pool: &SqlitePool, id: i64) -> sqlx::Result<bool> {
    let result = sqlx::query!("DELETE FROM library WHERE id = ?", id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// List all libraries, sorted by name.
pub async fn list_libraries(pool: &SqlitePool) -> sqlx::Result<Vec<Library>> {
    sqlx::query_as!(Library, "SELECT id, name FROM library ORDER BY name")
        .fetch_all(pool)
        .await
}

pub async fn get_password_hash(pool: &SqlitePool) -> sqlx::Result<Option<String>> {
    let row = sqlx::query!("SELECT password_hash FROM password WHERE id = 1")
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|r| r.password_hash))
}

pub async fn insert_password_hash(pool: &SqlitePool, hash: &str) -> sqlx::Result<bool> {
    let result = sqlx::query!(
        "INSERT INTO password (id, password_hash) VALUES (1, ?) ON CONFLICT DO NOTHING",
        hash
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Update an existing password hash
///
/// Does not upsert the password hash if it's missing. This is so that two racing clients both
/// attempting to set the password cannot both succeed.
pub async fn update_password_hash(pool: &SqlitePool, hash: &str) -> sqlx::Result<()> {
    sqlx::query!("UPDATE password SET password_hash = ? WHERE id = 1", hash)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;

    use super::*;

    #[tokio::test]
    async fn create_and_migrate_db() {
        let tmp = tempfile::tempdir().unwrap();
        // A directory that doesn't exist yet, to show connect() creates it
        let data_dir = tmp.path().join("data");

        let pool = connect(&data_dir).await.unwrap();

        assert!(data_dir.join("scorarium.db").exists());
        // The schema exists, i.e. migrations ran
        assert_eq!(list_libraries(&pool).await.unwrap().len(), 0);
    }

    #[sqlx::test]
    async fn create_and_list_libraries(pool: SqlitePool) {
        create_library(&pool, "lib2").await.unwrap();
        create_library(&pool, "lib1").await.unwrap();

        let libraries = list_libraries(&pool).await.unwrap();
        let names: Vec<&str> = libraries.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["lib1", "lib2"]);
    }
}
