pub mod pending_import;
pub mod person;
pub mod publication;
pub mod work;

use sqlx::SqlitePool;

/// Create a new library, returning its id.
//
// TODO: Remove when the tests no longer need to create libraries themselves
pub async fn create_library(pool: &SqlitePool, name: &str) -> sqlx::Result<i64> {
    let result = sqlx::query!("INSERT INTO library (name) VALUES (?)", name)
        .execute(pool)
        .await?;
    Ok(result.last_insert_rowid())
}
