use std::sync::Arc;

use sqlx::SqliteConnection;

use crate::holding::HoldingInput;
use crate::import::{self, PendingImport};
use crate::person::{self, Person};
use crate::publication::{self, Publication, PublicationInput};
use crate::work::{self, Work};
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
        let mut tx = self.archive.pool.begin().await?;
        // The pending imports cascade away with the library, but their drafts are in memory.
        let drafted = import::pending_import_ids(&mut tx, self.id).await?;
        let result = sqlx::query!("DELETE FROM library WHERE id = ?", self.id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Err(NotFound.into());
        }
        tx.commit().await?;
        let mut drafts = self.archive.drafts.lock().expect("draft lock poisoned");
        for id in drafted {
            drafts.remove(&id);
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

// imports
impl Library {
    /// This library's pending imports, oldest first
    pub async fn pending_imports(&self) -> Result<Vec<PendingImport>> {
        let mut tx = self.archive.pool.begin().await?;
        let imports =
            import::load_pending_imports(&self.archive, &mut tx, Some(self.id), None).await?;
        tx.commit().await?;
        Ok(imports)
    }

    /// The given pending import, if this library has it
    pub async fn pending_import(&self, id: i64) -> Result<Option<PendingImport>> {
        let mut tx = self.archive.pool.begin().await?;
        let import = import::load_pending_imports(&self.archive, &mut tx, Some(self.id), Some(id))
            .await?
            .pop();
        tx.commit().await?;
        Ok(import)
    }

    /// Start an import from what the entry page collected
    pub async fn start_import(
        &self,
        query: &str,
        holdings: &[HoldingInput],
    ) -> Result<PendingImport> {
        let mut tx = self.archive.pool.begin().await?;
        let import = import::start_import(&self.archive, &mut tx, self.id, query, holdings).await?;
        tx.commit().await?;
        Ok(import)
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

    /// Fold one work into another and delete it; the survivor is returned reloaded.
    ///
    /// Returns a [NotFound] error when either work is not in this library.
    pub async fn merge_works(&self, from: i64, into: i64) -> Result<Work> {
        let mut tx = self.archive.pool.begin().await?;
        let both = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM work WHERE library_id = ?1 AND id IN (?2, ?3)",
            self.id,
            from,
            into
        )
        .fetch_one(&mut *tx)
        .await?;
        if both != 2 {
            tx.rollback().await?;
            return Err(NotFound.into());
        }
        work::merge_works(&mut tx, self.id, from, into).await?;
        collect_orphans(&mut tx, self.id).await?;
        let survivor = work::load_works(&self.archive, &mut tx, self.id, Some(into), None)
            .await?
            .pop()
            .expect("the survivor was just verified on this transaction");
        tx.commit().await?;
        Ok(survivor)
    }
}

// people
impl Library {
    /// The given person, if this library has them
    pub async fn person(&self, id: i64) -> Result<Option<Person>> {
        let mut conn = self.archive.pool.acquire().await?;
        person::get_person(&self.archive, &mut conn, self.id, id).await
    }

    /// Everyone credited with `role` on any publication or work here, by sort name
    pub async fn persons_with_role(&self, role: &str) -> Result<Vec<Person>> {
        let mut conn = self.archive.pool.acquire().await?;
        person::list_persons_with_role(&self.archive, &mut conn, self.id, role).await
    }

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
