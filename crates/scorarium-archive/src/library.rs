use std::sync::Arc;

use sqlx::SqliteConnection;

use crate::audit::Audited;
use crate::holding::HoldingInput;
use crate::import::{self, PendingImport};
use crate::person::{self, Person};
use crate::publication::{self, Publication, PublicationInput};
use crate::work::{self, CatalogNumberEntry, Work};
use crate::{Action, ArchiveInner, EntityKind, EntityRef, Event, NotFound, Result, Source};

/// A named container of publications.
#[derive(Clone, Debug)]
pub struct Library {
    pub id: i64,
    pub name: String,
    pub private: bool,
    archive: Arc<ArchiveInner>,
}

impl Library {
    /// Rename the given library.
    ///
    /// Returns a [NotFound] error if the library this [Library] handle refers to has been deleted.
    pub async fn rename(&mut self, name: &str) -> Result<()> {
        // The entry carries the name the library was renamed to
        let entity = EntityRef {
            label: name.to_string(),
            ..self.entity_ref()
        };
        let mut audited = self
            .archive
            .begin_audit(Source::User, Event::about(Action::Renamed, entity))
            .await?;
        let renamed = sqlx::query_scalar!(
            r#"UPDATE library SET name = ? WHERE id = ? RETURNING name AS "name!""#,
            name,
            self.id
        )
        .fetch_optional(&mut *audited)
        .await?;
        let Some(renamed) = renamed else {
            audited.rollback().await?;
            return Err(NotFound.into());
        };
        audited.commit().await?;
        self.name = renamed;
        Ok(())
    }

    /// Delete this library and everything in it.
    ///
    /// Returns a [NotFound] error if the library this [Library] handle refers to has been deleted.
    pub async fn delete(self) -> Result<()> {
        let mut audited = self
            .archive
            .begin_audit(
                Source::User,
                Event::about(Action::Deleted, self.entity_ref()),
            )
            .await?;
        // The pending imports cascade away with the library, but their drafts are in memory.
        let drafted = import::pending_import_ids(&mut audited, self.id).await?;
        let result = sqlx::query!("DELETE FROM library WHERE id = ?", self.id)
            .execute(&mut *audited)
            .await?;
        if result.rows_affected() == 0 {
            audited.rollback().await?;
            return Err(NotFound.into());
        }
        audited.commit().await?;
        let mut drafts = self.archive.drafts.lock().expect("draft lock poisoned");
        for id in drafted {
            drafts.remove(&id);
        }
        Ok(())
    }

    /// Get an EntityRef referring to this entity for use in the audit log
    pub(crate) fn entity_ref(&self) -> EntityRef {
        EntityRef {
            kind: EntityKind::Library,
            id: self.id,
            library_id: Some(self.id),
            label: self.name.clone(),
        }
    }
}

// publications
impl Library {
    /// Every publication in the library, in arbitrary order
    pub async fn publications(&self) -> Result<Vec<Publication>> {
        let mut tx = self.archive.begin_read().await?;
        let publications =
            publication::load_publications(&self.archive, &mut tx, self.id, None, None, None)
                .await?;
        tx.commit().await?;
        Ok(publications)
    }

    /// The given publication, if this library has it
    pub async fn publication(&self, id: i64) -> Result<Option<Publication>> {
        let mut tx = self.archive.begin_read().await?;
        let publication =
            publication::load_publications(&self.archive, &mut tx, self.id, Some(id), None, None)
                .await?
                .pop();
        tx.commit().await?;
        Ok(publication)
    }

    /// Create a publication, with its identifiers, contributors, holdings and contents
    pub async fn create_publication(&self, input: &PublicationInput) -> Result<Publication> {
        let mut audited = self
            .archive
            .begin_audit(Source::User, Event::new(Action::Created))
            .await?;
        let publication =
            publication::create_publication(&self.archive, &mut audited, self.id, input).await?;
        audited.set_entity(&publication.entity_ref()).await?;
        audited.commit().await?;
        Ok(publication)
    }
}

// imports
impl Library {
    /// This library's pending imports, oldest first
    pub async fn pending_imports(&self) -> Result<Vec<PendingImport>> {
        let mut tx = self.archive.begin_read().await?;
        let imports =
            import::load_pending_imports(&self.archive, &mut tx, Some(self.id), None).await?;
        tx.commit().await?;
        Ok(imports)
    }

    /// The given pending import, if this library has it
    pub async fn pending_import(&self, id: i64) -> Result<Option<PendingImport>> {
        let mut tx = self.archive.begin_read().await?;
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
        let mut audited = self
            .archive
            .begin_audit(Source::User, Event::new(Action::ImportStarted))
            .await?;
        let import =
            import::start_import(&self.archive, &mut audited, self.id, query, holdings).await?;
        audited.set_entity(&import.entity_ref()).await?;
        audited.commit().await?;
        Ok(import)
    }
}

// works
impl Library {
    /// The given work, if this library has it
    pub async fn work(&self, id: i64) -> Result<Option<Work>> {
        let mut tx = self.archive.begin_read().await?;
        let work = work::load_works(&self.archive, &mut tx, self.id, Some(id), None)
            .await?
            .pop();
        tx.commit().await?;
        Ok(work)
    }

    /// Every catalog number in the library, optionally only those credited to one composer
    pub async fn catalog_numbers(&self, composer: Option<&str>) -> Result<Vec<CatalogNumberEntry>> {
        let mut conn = self.archive.acquire_read().await?;
        work::load_catalog_numbers(&mut conn, Some(self.id), composer).await
    }

    /// Fold one work into another and delete it; the survivor is returned reloaded.
    ///
    /// Returns a [NotFound] error when either work is not in this library.
    pub async fn merge_works(&self, from: i64, into: i64) -> Result<Work> {
        let mut audited = self
            .archive
            .begin_audit(Source::User, Event::new(Action::Merged))
            .await?;
        let both = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM work WHERE library_id = ?1 AND id IN (?2, ?3)",
            self.id,
            from,
            into
        )
        .fetch_one(&mut *audited)
        .await?;
        if both != 2 {
            audited.rollback().await?;
            return Err(NotFound.into());
        }
        work::merge_works(&mut audited, self.id, from, into).await?;
        collect_orphans(&mut audited, self.id).await?;
        let survivor = work::load_works(&self.archive, &mut audited, self.id, Some(into), None)
            .await?
            .pop()
            .expect("the survivor was just verified on this transaction");
        audited.set_entity(&survivor.entity_ref()).await?;
        audited.commit().await?;
        Ok(survivor)
    }
}

// people
impl Library {
    /// The given person, if this library has them
    pub async fn person(&self, id: i64) -> Result<Option<Person>> {
        let mut conn = self.archive.acquire_read().await?;
        person::get_person(&self.archive, &mut conn, self.id, id).await
    }

    /// Everyone credited with `role` on any publication or work here, by sort name
    pub async fn persons_with_role(&self, role: &str) -> Result<Vec<Person>> {
        let mut conn = self.archive.acquire_read().await?;
        person::list_persons_with_role(&self.archive, &mut conn, self.id, role).await
    }

    /// The distinct roles credited anywhere in the library, sorted, for input suggestions
    pub async fn roles(&self) -> Result<Vec<String>> {
        let mut conn = self.archive.acquire_read().await?;
        person::list_contributor_roles(&mut conn, self.id).await
    }

    /// Every person's display name, by sort name, for input suggestions
    pub async fn person_names(&self) -> Result<Vec<String>> {
        let mut conn = self.archive.acquire_read().await?;
        person::list_person_names(&mut conn, self.id).await
    }
}

/// Delete what the library no longer has reachable links to.
///
/// The order matters: collecting a work takes its contributor links with it, and those links can
/// be the last thing crediting a person.
pub(crate) async fn collect_orphans(audited: &mut Audited<'_>, library_id: i64) -> Result<()> {
    let works = sqlx::query!(
        r#"DELETE FROM work
           WHERE library_id = ? AND id NOT IN (SELECT work_id FROM publication_work)
           RETURNING id AS "id!", title AS "title!""#,
        library_id
    )
    .fetch_all(&mut **audited)
    .await?;
    for work in works {
        let entity = EntityRef {
            kind: EntityKind::Work,
            id: work.id,
            library_id: Some(library_id),
            label: work.title,
        };
        audited
            .record(Source::OrphanCleanup, Event::about(Action::Deleted, entity))
            .await?;
    }
    let persons = sqlx::query!(
        r#"DELETE FROM person
           WHERE library_id = ?
             AND id NOT IN (SELECT person_id FROM publication_contributor)
             AND id NOT IN (SELECT person_id FROM work_contributor)
           RETURNING id AS "id!", name AS "name!""#,
        library_id
    )
    .fetch_all(&mut **audited)
    .await?;
    for person in persons {
        let entity = EntityRef {
            kind: EntityKind::Person,
            id: person.id,
            library_id: Some(library_id),
            label: person.name,
        };
        audited
            .record(Source::OrphanCleanup, Event::about(Action::Deleted, entity))
            .await?;
    }
    Ok(())
}

pub(crate) async fn list_libraries(
    shared: &Arc<ArchiveInner>,
    conn: &mut SqliteConnection,
) -> Result<Vec<Library>> {
    let rows =
        sqlx::query!(r#"SELECT id, name, private AS "private: bool" FROM library ORDER BY name"#)
            .fetch_all(conn)
            .await?;
    Ok(rows
        .into_iter()
        .map(|row| Library {
            id: row.id,
            name: row.name,
            private: row.private,
            archive: shared.clone(),
        })
        .collect())
}

pub(crate) async fn get_library(
    shared: &Arc<ArchiveInner>,
    conn: &mut SqliteConnection,
    id: i64,
) -> Result<Option<Library>> {
    let row = sqlx::query!(
        r#"SELECT id, name, private AS "private: bool" FROM library WHERE id = ?"#,
        id
    )
    .fetch_optional(conn)
    .await?;
    Ok(row.map(|row| Library {
        id: row.id,
        name: row.name,
        private: row.private,
        archive: shared.clone(),
    }))
}

pub(crate) async fn create_library(
    shared: &Arc<ArchiveInner>,
    conn: &mut SqliteConnection,
    name: &str,
    private: bool,
) -> Result<Library> {
    let id = sqlx::query!(
        "INSERT INTO library (name, private) VALUES (?, ?)",
        name,
        private
    )
    .execute(conn)
    .await?
    .last_insert_rowid();
    Ok(Library {
        id,
        name: name.to_string(),
        private,
        archive: shared.clone(),
    })
}
