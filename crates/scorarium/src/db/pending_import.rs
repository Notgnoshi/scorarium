use std::collections::HashMap;

use sqlx::{SqliteExecutor, SqlitePool};

use crate::db::publication::{HoldingKind, decode_error};

/// An import the user has started but not yet accepted or discarded
#[derive(Debug, PartialEq, Eq)]
pub struct PendingImport {
    pub id: i64,
    pub library_id: i64,
    pub library_name: String,
    pub query: String,
    /// In the order entered, which is the order the review page shows them
    pub holdings: Vec<PendingHolding>,
    /// Unix seconds
    pub created_at: i64,
}

/// A copy entered on the entry page; becomes a holding on accept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingHolding {
    pub kind: HoldingKind,
    /// Freeform for physical, a file path for digital
    pub location: Option<String>,
}

pub struct NewPendingImport<'a> {
    pub library_id: i64,
    pub query: &'a str,
    pub holdings: &'a [PendingHolding],
}

pub async fn create(pool: &SqlitePool, new: &NewPendingImport<'_>) -> sqlx::Result<i64> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query!(
        "INSERT INTO pending_import (library_id, query) VALUES (?, ?)",
        new.library_id,
        new.query,
    )
    .execute(&mut *tx)
    .await?;
    let id = result.last_insert_rowid();
    for holding in new.holdings {
        let kind = holding.kind.as_str();
        sqlx::query!(
            "INSERT INTO pending_import_holding (pending_import_id, kind, location) VALUES (?, ?, ?)",
            id,
            kind,
            holding.location,
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(id)
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
    let mut tx = pool.begin().await?;
    let mut imports: Vec<PendingImport> = sqlx::query!(
        "SELECT p.id, p.library_id, l.name AS library_name, p.query, p.created_at
         FROM pending_import p JOIN library l ON l.id = p.library_id
         WHERE (?1 IS NULL OR p.library_id = ?1) AND (?2 IS NULL OR p.id = ?2)
         ORDER BY p.created_at, p.id",
        library_id,
        id
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|row| PendingImport {
        id: row.id,
        library_id: row.library_id,
        library_name: row.library_name,
        query: row.query,
        holdings: Vec::new(),
        created_at: row.created_at,
    })
    .collect();
    let index: HashMap<i64, usize> = imports.iter().enumerate().map(|(i, p)| (p.id, i)).collect();

    let holdings = sqlx::query!(
        "SELECT pending_import_id, kind, location FROM pending_import_holding
         WHERE pending_import_id IN
            (SELECT id FROM pending_import
             WHERE (?1 IS NULL OR library_id = ?1) AND (?2 IS NULL OR id = ?2))
         ORDER BY id",
        library_id,
        id
    )
    .fetch_all(&mut *tx)
    .await?;
    for row in holdings {
        imports[index[&row.pending_import_id]]
            .holdings
            .push(PendingHolding {
                kind: row.kind.parse().map_err(decode_error)?,
                location: row.location,
            });
    }
    tx.commit().await?;
    Ok(imports)
}

/// How many imports await review, for the header.
pub async fn count(pool: &SqlitePool) -> sqlx::Result<i64> {
    sqlx::query_scalar!("SELECT COUNT(*) FROM pending_import")
        .fetch_one(pool)
        .await
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
