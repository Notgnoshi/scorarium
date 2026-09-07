use std::sync::Arc;

use sqlx::SqliteConnection;

use crate::{ArchiveInner, NotFound, Result};

/// A named container of publications.
#[derive(Clone, Debug)]
pub struct Library {
    pub id: i64,
    pub name: String,
    archive: Arc<ArchiveInner>,
}

impl Library {
    /// Rename the given library.
    ///
    /// Returns a [NotFound] error if the library this [Library] handle refers to has been deleted.
    pub async fn rename(&mut self, name: &str) -> Result<()> {
        let renamed = sqlx::query_scalar!(
            r#"UPDATE library SET name = ? WHERE id = ? RETURNING name AS "name!""#,
            name,
            self.id
        )
        .fetch_optional(&self.archive.pool)
        .await?;
        self.name = renamed.ok_or(NotFound)?;
        Ok(())
    }

    /// Delete this library and everything in it.
    ///
    /// Returns a [NotFound] error if the library this [Library] handle refers to has been deleted.
    pub async fn delete(self) -> Result<()> {
        let result = sqlx::query!("DELETE FROM library WHERE id = ?", self.id)
            .execute(&self.archive.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(NotFound.into());
        }
        Ok(())
    }
}

pub(crate) async fn list(
    shared: &Arc<ArchiveInner>,
    conn: &mut SqliteConnection,
) -> Result<Vec<Library>> {
    let rows = sqlx::query!("SELECT id, name FROM library ORDER BY name")
        .fetch_all(conn)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| Library {
            id: row.id,
            name: row.name,
            archive: shared.clone(),
        })
        .collect())
}

pub(crate) async fn get(
    shared: &Arc<ArchiveInner>,
    conn: &mut SqliteConnection,
    id: i64,
) -> Result<Option<Library>> {
    let row = sqlx::query!("SELECT id, name FROM library WHERE id = ?", id)
        .fetch_optional(conn)
        .await?;
    Ok(row.map(|row| Library {
        id: row.id,
        name: row.name,
        archive: shared.clone(),
    }))
}

pub(crate) async fn create(
    shared: &Arc<ArchiveInner>,
    conn: &mut SqliteConnection,
    name: &str,
) -> Result<Library> {
    let id = sqlx::query!("INSERT INTO library (name) VALUES (?)", name)
        .execute(conn)
        .await?
        .last_insert_rowid();
    Ok(Library {
        id,
        name: name.to_string(),
        archive: shared.clone(),
    })
}
