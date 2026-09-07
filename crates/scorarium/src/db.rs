pub mod pending_import;
pub mod person;
pub mod publication;
pub mod work;

use sqlx::SqlitePool;

/// Create a new library, returning its id.
//
// TODO: Remove once the demo database gets migrated over to the new Archive
pub async fn create_library(pool: &SqlitePool, name: &str) -> sqlx::Result<i64> {
    let result = sqlx::query!("INSERT INTO library (name) VALUES (?)", name)
        .execute(pool)
        .await?;
    Ok(result.last_insert_rowid())
}
