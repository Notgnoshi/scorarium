use sqlx::{SqliteExecutor, SqlitePool};

use crate::db::publication::{HoldingKind, decode_error};

/// An import the user has started but not yet accepted or discarded
#[derive(Debug, PartialEq, Eq)]
pub struct PendingImport {
    pub id: i64,
    pub library_id: i64,
    pub library_name: String,
    pub query: String,
    pub kind: HoldingKind,
    pub location: Option<String>,
    /// Unix seconds
    pub created_at: i64,
}

pub struct NewPendingImport<'a> {
    pub library_id: i64,
    pub query: &'a str,
    pub kind: HoldingKind,
    pub location: Option<&'a str>,
}

pub async fn create(
    executor: impl SqliteExecutor<'_>,
    new: &NewPendingImport<'_>,
) -> sqlx::Result<i64> {
    let kind = new.kind.as_str();
    let result = sqlx::query!(
        "INSERT INTO pending_import (library_id, query, kind, location) VALUES (?, ?, ?, ?)",
        new.library_id,
        new.query,
        kind,
        new.location,
    )
    .execute(executor)
    .await?;
    Ok(result.last_insert_rowid())
}

/// One pending import, or None when it does not exist or belongs to another library.
pub async fn get(
    pool: &SqlitePool,
    library_id: i64,
    id: i64,
) -> sqlx::Result<Option<PendingImport>> {
    Ok(load(pool, Some(library_id), Some(id)).await?.pop())
}

/// Pending imports in one library, or in all of them, oldest first
pub async fn list(pool: &SqlitePool, library_id: Option<i64>) -> sqlx::Result<Vec<PendingImport>> {
    load(pool, library_id, None).await
}

async fn load(
    pool: &SqlitePool,
    library_id: Option<i64>,
    id: Option<i64>,
) -> sqlx::Result<Vec<PendingImport>> {
    sqlx::query!(
        "SELECT p.id, p.library_id, l.name AS library_name, p.query, p.kind, p.location, p.created_at
         FROM pending_import p JOIN library l ON l.id = p.library_id
         WHERE (?1 IS NULL OR p.library_id = ?1) AND (?2 IS NULL OR p.id = ?2)
         ORDER BY p.created_at, p.id",
        library_id,
        id
    )
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|row| {
        Ok(PendingImport {
            id: row.id,
            library_id: row.library_id,
            library_name: row.library_name,
            query: row.query,
            kind: row.kind.parse().map_err(decode_error)?,
            location: row.location,
            created_at: row.created_at,
        })
    })
    .collect()
}

/// Returns false when no pending import matches
pub async fn delete(
    executor: impl SqliteExecutor<'_>,
    library_id: i64,
    id: i64,
) -> sqlx::Result<bool> {
    let result = sqlx::query!(
        "DELETE FROM pending_import WHERE library_id = ? AND id = ?",
        library_id,
        id
    )
    .execute(executor)
    .await?;
    Ok(result.rows_affected() > 0)
}
