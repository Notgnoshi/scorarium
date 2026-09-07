use std::sync::Arc;

use sqlx::SqliteConnection;

use crate::publication::{self, Publication, PublicationInput};
use crate::work::{self, Work};
use crate::{ArchiveInner, NotFound, Result, person};

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

// publications
impl Library {
    /// Every publication in the library, in arbitrary order
    pub async fn publications(&self) -> Result<Vec<Publication>> {
        let mut tx = self.archive.pool.begin().await?;
        let publications =
            publication::load_publications(&self.archive, &mut tx, self.id, None, None, None)
                .await?;
        tx.commit().await?;
        Ok(publications)
    }

    /// The given publication, if this library has it
    pub async fn publication(&self, id: i64) -> Result<Option<Publication>> {
        let mut tx = self.archive.pool.begin().await?;
        let publication =
            publication::load_publications(&self.archive, &mut tx, self.id, Some(id), None, None)
                .await?
                .pop();
        tx.commit().await?;
        Ok(publication)
    }

    /// Create a publication, with its identifiers, contributors, holdings and contents
    pub async fn create_publication(&self, input: &PublicationInput) -> Result<Publication> {
        let mut tx = self.archive.pool.begin().await?;
        let publication =
            publication::create_publication(&self.archive, &mut tx, self.id, input).await?;
        tx.commit().await?;
        Ok(publication)
    }
}

// works
impl Library {
    /// The given work, if this library has it
    pub async fn work(&self, id: i64) -> Result<Option<Work>> {
        let mut tx = self.archive.pool.begin().await?;
        let work = work::load_works(&self.archive, &mut tx, self.id, Some(id), None)
            .await?
            .pop();
        tx.commit().await?;
        Ok(work)
    }
}

// people
impl Library {
    /// The distinct roles credited anywhere in the library, sorted, for input suggestions
    pub async fn roles(&self) -> Result<Vec<String>> {
        let mut conn = self.archive.pool.acquire().await?;
        person::list_contributor_roles(&mut conn, self.id).await
    }

    /// Every person's display name, by sort name, for input suggestions
    pub async fn person_names(&self) -> Result<Vec<String>> {
        let mut conn = self.archive.pool.acquire().await?;
        person::list_person_names(&mut conn, self.id).await
    }
}

/// Delete what the library no longer has reachable links to.
///
/// The order matters: collecting a work takes its contributor links with it, and those links can
/// be the last thing crediting a person.
pub(crate) async fn collect_orphans(conn: &mut SqliteConnection, library_id: i64) -> Result<()> {
    sqlx::query!(
        "DELETE FROM work
         WHERE library_id = ? AND id NOT IN (SELECT work_id FROM publication_work)",
        library_id
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        "DELETE FROM person
         WHERE library_id = ?
           AND id NOT IN (SELECT person_id FROM publication_contributor)
           AND id NOT IN (SELECT person_id FROM work_contributor)",
        library_id
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub(crate) async fn list_libraries(
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

pub(crate) async fn get_library(
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

pub(crate) async fn create_library(
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
