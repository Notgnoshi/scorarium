use std::cmp::Ordering;
use std::fmt;
use std::num::ParseIntError;
use std::str::FromStr;

use sqlx::SqliteConnection;

use crate::audit::{Action, AuditEvent, AuditId, AuditSource, AuditSubject, Audited};
use crate::contributor::{Contributor, clean_role};
use crate::entity::{Entity, EntityKind, Timestamp};
use crate::library::LibraryId;
use crate::person::{self, Person, PersonId};
use crate::publication::{self, Publication, PublicationId};
use crate::value::{Classification, Stars, clean_text};
use crate::{Engine, Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WorkId(i64);

impl WorkId {
    pub(crate) fn new(id: i64) -> WorkId {
        WorkId(id)
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

impl fmt::Display for WorkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for WorkId {
    type Err = ParseIntError;

    fn from_str(text: &str) -> std::result::Result<WorkId, ParseIntError> {
        text.parse().map(WorkId)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    Title,
    Key,
    TimeSignature,
    Instrumentation,
    Stars,
    Note,
    Classification,
    Tags,
    CatalogNumbers,
    Links,
    Contributors,
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Field::Title => "title",
            Field::Key => "key",
            Field::TimeSignature => "time_signature",
            Field::Instrumentation => "instrumentation",
            Field::Stars => "stars",
            Field::Note => "note",
            Field::Classification => "classification",
            Field::Tags => "tags",
            Field::CatalogNumbers => "catalog_numbers",
            Field::Links => "links",
            Field::Contributors => "contributors",
        })
    }
}

impl FromStr for Field {
    type Err = Error;

    fn from_str(text: &str) -> Result<Field> {
        Ok(match text {
            "title" => Field::Title,
            "key" => Field::Key,
            "time_signature" => Field::TimeSignature,
            "instrumentation" => Field::Instrumentation,
            "stars" => Field::Stars,
            "note" => Field::Note,
            "classification" => Field::Classification,
            "tags" => Field::Tags,
            "catalog_numbers" => Field::CatalogNumbers,
            "links" => Field::Links,
            "contributors" => Field::Contributors,
            other => return Err(eyre::eyre!("unknown work field {other:?}").into()),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Work {
    engine: Engine,
    id: WorkId,
    library: LibraryId,
    draft: bool,
    created_at: Timestamp,
    last_modified_at: Timestamp,
    title: String,
    key: Option<String>,
    time_signature: Option<String>,
    instrumentation: Option<String>,
    stars: Option<Stars>,
    note: Option<String>,
    classification: Option<Classification>,
    contributors: Vec<Contributor>,
}

/// A POD type to serialize a work from the database into
struct WorkRow {
    id: i64,
    library_id: i64,
    title: String,
    key: Option<String>,
    time_signature: Option<String>,
    instrumentation: Option<String>,
    stars: Option<u8>,
    note: Option<String>,
    kind: Option<String>,
    draft: bool,
    created_at: Timestamp,
    last_modified_at: Timestamp,
}

impl Work {
    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    pub fn time_signature(&self) -> Option<&str> {
        self.time_signature.as_deref()
    }

    pub fn instrumentation(&self) -> Option<&str> {
        self.instrumentation.as_deref()
    }

    pub fn stars(&self) -> Option<Stars> {
        self.stars
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    pub fn classification(&self) -> Option<Classification> {
        self.classification
    }

    pub fn contributors(&self) -> &[Contributor] {
        &self.contributors
    }

    async fn from_row(engine: &Engine, conn: &mut SqliteConnection, row: WorkRow) -> Result<Work> {
        let contributors = sqlx::query!(
            "SELECT work_contributor.person_id, person.name, work_contributor.role
             FROM work_contributor JOIN person ON person.id = work_contributor.person_id
             WHERE work_contributor.work_id = ?
             ORDER BY work_contributor.id",
            row.id
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|contributor| Contributor {
            person: PersonId::new(contributor.person_id),
            name: contributor.name,
            role: contributor.role,
        })
        .collect();
        Ok(Work {
            engine: engine.clone(),
            id: WorkId::new(row.id),
            library: LibraryId::new(row.library_id),
            draft: row.draft,
            created_at: row.created_at,
            last_modified_at: row.last_modified_at,
            title: row.title,
            key: row.key,
            time_signature: row.time_signature,
            instrumentation: row.instrumentation,
            stars: row.stars.map(Stars::try_from).transpose()?,
            note: row.note,
            classification: row.kind.as_deref().map(str::parse).transpose()?,
            contributors,
        })
    }

    async fn fetch(
        engine: &Engine,
        conn: &mut SqliteConnection,
        id: WorkId,
    ) -> Result<Option<Work>> {
        let row = sqlx::query_as!(
            WorkRow,
            r#"SELECT id, library_id, title, key, time_signature, instrumentation,
                      stars AS "stars: u8", note, kind, draft AS "draft: bool",
                      created_at AS "created_at: Timestamp",
                      last_modified_at AS "last_modified_at: Timestamp"
               FROM work WHERE id = ?"#,
            id.get()
        )
        .fetch_optional(&mut *conn)
        .await?;
        match row {
            Some(row) => Ok(Some(Work::from_row(engine, conn, row).await?)),
            None => Ok(None),
        }
    }

    async fn refresh(&mut self, conn: &mut SqliteConnection) -> Result<()> {
        *self = Work::fetch(&self.engine, conn, self.id)
            .await?
            .ok_or(Error::NotFound)?;
        Ok(())
    }

    pub(crate) fn audit_subject(&self) -> AuditSubject {
        audit_subject(self.id, self.library, &self.title)
    }

    async fn commit_update(&self, tx: Audited<'_>, field: Field) -> Result<()> {
        tx.record(
            AuditSource::User,
            Action::Updated,
            Some(&self.audit_subject()),
            Some(&field.to_string()),
        )
        .await?
        .commit()
        .await
    }

    /// Retitle the work
    ///
    /// Returns [Error::NotFound] if the work has been deleted.
    pub async fn set_title(&mut self, title: impl AsRef<str>) -> Result<()> {
        let title = clean_title(title.as_ref())?;
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.title == title {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE work SET title = ?, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            title,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Title).await?;
        self.title = title.to_string();
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Set or clear the key, treating a blank key as none
    ///
    /// Returns [Error::NotFound] if the work has been deleted.
    pub async fn set_key(&mut self, key: Option<&str>) -> Result<()> {
        let key = clean_text(key);
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.key.as_deref() == key {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE work SET key = ?, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            key,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Key).await?;
        self.key = key.map(str::to_string);
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Set or clear the time signature, treating a blank time signature as none
    ///
    /// Returns [Error::NotFound] if the work has been deleted.
    pub async fn set_time_signature(&mut self, time_signature: Option<&str>) -> Result<()> {
        let time_signature = clean_text(time_signature);
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.time_signature.as_deref() == time_signature {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE work SET time_signature = ?, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            time_signature,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::TimeSignature).await?;
        self.time_signature = time_signature.map(str::to_string);
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Set or clear the instrumentation, treating a blank instrumentation as none
    ///
    /// Returns [Error::NotFound] if the work has been deleted.
    pub async fn set_instrumentation(&mut self, instrumentation: Option<&str>) -> Result<()> {
        let instrumentation = clean_text(instrumentation);
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.instrumentation.as_deref() == instrumentation {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE work SET instrumentation = ?, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            instrumentation,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Instrumentation).await?;
        self.instrumentation = instrumentation.map(str::to_string);
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Set or clear the rating
    ///
    /// Returns [Error::NotFound] if the work has been deleted.
    pub async fn set_stars(&mut self, stars: Option<Stars>) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.stars == stars {
            return Ok(());
        }
        let value = stars.map(Stars::get);
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE work SET stars = ?, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            value,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Stars).await?;
        self.stars = stars;
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Set or clear the note, treating a blank note as none
    ///
    /// Returns [Error::NotFound] if the work has been deleted.
    pub async fn set_note(&mut self, note: Option<&str>) -> Result<()> {
        let note = clean_text(note);
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.note.as_deref() == note {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE work SET note = ?, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            note,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Note).await?;
        self.note = note.map(str::to_string);
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Classify the work as music or prose
    ///
    /// Returns [Error::NotFound] if the work has been deleted.
    pub async fn set_classification(&mut self, classification: Classification) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.classification == Some(classification) {
            return Ok(());
        }
        let kind = classification.to_string();
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE work SET kind = ?, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            kind,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Classification).await?;
        self.classification = Some(classification);
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// List every publication that contains the work
    pub async fn publications(&self) -> Result<Vec<Publication>> {
        publication::containing(&self.engine, self.id).await
    }

    /// Create a person with the owning work's draft state
    ///
    /// Returns [Error::NotFound] if the work has been deleted.
    pub async fn create_contributor(
        &mut self,
        name: impl AsRef<str>,
        role: impl AsRef<str>,
    ) -> Result<Person> {
        let name = person::clean_name(name.as_ref())?;
        let role = clean_role(role.as_ref())?;
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let person = person::insert(&self.engine, &mut tx, self.library, name, self.draft).await?;
        sqlx::query!(
            "INSERT INTO work_contributor (library_id, work_id, person_id, role) VALUES (?, ?, ?, ?)",
            self.library.get(),
            self.id.get(),
            person.id().get(),
            role
        )
        .execute(&mut *tx)
        .await?;
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE work SET last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        tx.record(
            AuditSource::User,
            Action::Created,
            Some(&person.audit_subject()),
            None,
        )
        .await?
        .record(
            AuditSource::User,
            Action::Updated,
            Some(&self.audit_subject()),
            Some(&Field::Contributors.to_string()),
        )
        .await?
        .commit()
        .await?;
        self.contributors.push(Contributor {
            person: person.id(),
            name: person.name().to_string(),
            role: role.to_string(),
        });
        self.last_modified_at = last_modified_at;
        Ok(person)
    }

    /// Add a person from the work's library as a contributor in `role`
    ///
    /// Commit the person if it is a draft and the work is committed.
    ///
    /// Returns [Error::NotFound] if the work has been deleted or its library has no such person.
    pub async fn add_contributor(&mut self, person: PersonId, role: impl AsRef<str>) -> Result<()> {
        let role = clean_role(role.as_ref())?;
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let name = person::name(&mut tx, self.library, person)
            .await?
            .ok_or(Error::NotFound)?;
        let inserted = sqlx::query!(
            "INSERT INTO work_contributor (library_id, work_id, person_id, role)
             VALUES (?, ?, ?, ?)
             ON CONFLICT (work_id, person_id, role) DO NOTHING",
            self.library.get(),
            self.id.get(),
            person.get(),
            role
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if inserted == 0 {
            return Ok(());
        }
        let committed = if self.draft {
            Vec::new()
        } else {
            person::commit_contributors_of_work(&mut tx, self.id).await?
        };
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE work SET last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        let mut tx = tx
            .record(
                AuditSource::User,
                Action::Updated,
                Some(&self.audit_subject()),
                Some(&Field::Contributors.to_string()),
            )
            .await?;
        for subject in committed {
            tx = tx
                .record(AuditSource::User, Action::Committed, Some(&subject), None)
                .await?;
        }
        tx.commit().await?;
        self.contributors.push(Contributor {
            person,
            name,
            role: role.to_string(),
        });
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Remove a contributor from the work
    ///
    /// Returns [Error::NotFound] if the work has been deleted.
    pub async fn remove_contributor(
        &mut self,
        person: PersonId,
        role: impl AsRef<str>,
    ) -> Result<()> {
        let role = role.as_ref().trim();
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let removed = sqlx::query!(
            "DELETE FROM work_contributor WHERE work_id = ? AND person_id = ? AND role = ?",
            self.id.get(),
            person.get(),
            role
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if removed == 0 {
            return Ok(());
        }
        let people = person::collect_orphan_people(&mut tx, self.library).await?;
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE work SET last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        let mut tx = tx
            .record(
                AuditSource::User,
                Action::Updated,
                Some(&self.audit_subject()),
                Some(&Field::Contributors.to_string()),
            )
            .await?;
        for subject in people {
            tx = tx
                .record(
                    AuditSource::OrphanCleanup,
                    Action::Deleted,
                    Some(&subject),
                    None,
                )
                .await?;
        }
        tx.commit().await?;
        self.contributors
            .retain(|contributor| !(contributor.person == person && contributor.role == role));
        self.last_modified_at = last_modified_at;
        Ok(())
    }
}

impl Entity for Work {
    type Id = WorkId;
    type Field = Field;

    async fn load(engine: &Engine, id: WorkId) -> Result<Option<Work>> {
        let mut conn = engine.begin_read().await?;
        Work::fetch(engine, &mut conn, id).await
    }

    async fn reload(&mut self) -> Result<()> {
        let mut conn = self.engine.begin_read().await?;
        self.refresh(&mut conn).await
    }

    fn id(&self) -> WorkId {
        self.id
    }

    fn library(&self) -> LibraryId {
        self.library
    }

    fn is_draft(&self) -> bool {
        self.draft
    }

    fn created_at(&self) -> Timestamp {
        self.created_at
    }

    fn last_modified_at(&self) -> Timestamp {
        self.last_modified_at
    }

    async fn audit_log(&self, before: Option<AuditId>, limit: usize) -> Result<Vec<AuditEvent>> {
        self.engine
            .entity_audit_log(EntityKind::Work, self.id.get(), self.library, before, limit)
            .await
    }

    async fn commit(&mut self) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if !self.draft {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE work SET draft = 0, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        let people = person::commit_contributors_of_work(&mut tx, self.id).await?;
        let mut tx = tx
            .record(
                AuditSource::User,
                Action::Committed,
                Some(&self.audit_subject()),
                None,
            )
            .await?;
        for subject in people {
            tx = tx
                .record(AuditSource::User, Action::Committed, Some(&subject), None)
                .await?;
        }
        tx.commit().await?;
        self.draft = false;
        self.last_modified_at = last_modified_at;
        Ok(())
    }
}

/// Insert a work in `library` and load it
///
/// The caller owns the transaction and links the work to whatever references it.
pub(crate) async fn insert(
    engine: &Engine,
    conn: &mut SqliteConnection,
    library: LibraryId,
    title: &str,
    draft: bool,
    classification: Option<Classification>,
) -> Result<Work> {
    let library = library.get();
    let kind = classification.map(|classification| classification.to_string());
    let id = sqlx::query_scalar!(
        r#"INSERT INTO work (library_id, title, kind, draft) VALUES (?, ?, ?, ?)
           RETURNING id AS "id!""#,
        library,
        title,
        kind,
        draft
    )
    .fetch_one(&mut *conn)
    .await?;
    Work::fetch(engine, conn, WorkId::new(id))
        .await?
        .ok_or(Error::NotFound)
}

/// Check whether `library` has a work with the id `work`
pub(crate) async fn exists(
    conn: &mut SqliteConnection,
    library: LibraryId,
    work: WorkId,
) -> Result<bool> {
    let row = sqlx::query_scalar!(
        "SELECT id FROM work WHERE id = ? AND library_id = ?",
        work.get(),
        library.get()
    )
    .fetch_optional(conn)
    .await?;
    Ok(row.is_some())
}

/// Give the unclassified works in `publication` its `classification`
///
/// Return the audit subject of each work changed, in id order.
pub(crate) async fn inherit_classification(
    conn: &mut SqliteConnection,
    publication: PublicationId,
    classification: Classification,
) -> Result<Vec<AuditSubject>> {
    let kind = classification.to_string();
    let mut works = sqlx::query!(
        "UPDATE work SET kind = ?, last_modified_at = unixepoch()
         WHERE kind IS NULL
           AND id IN (SELECT work_id FROM publication_work WHERE publication_id = ?)
         RETURNING id, library_id, title",
        kind,
        publication.get()
    )
    .fetch_all(conn)
    .await?;
    // SQLite does not define the order of RETURNING rows
    works.sort_by_key(|work| work.id);
    Ok(works
        .into_iter()
        .map(|work| {
            audit_subject(
                WorkId::new(work.id),
                LibraryId::new(work.library_id),
                &work.title,
            )
        })
        .collect())
}

/// Clear the draft mark of every draft work in `publication`
///
/// Return the audit subject of each work committed, in id order.
pub(crate) async fn commit_works_of(
    conn: &mut SqliteConnection,
    publication: PublicationId,
) -> Result<Vec<AuditSubject>> {
    let mut works = sqlx::query!(
        "UPDATE work SET draft = 0, last_modified_at = unixepoch()
         WHERE draft = 1
           AND id IN (SELECT work_id FROM publication_work WHERE publication_id = ?)
         RETURNING id, library_id, title",
        publication.get()
    )
    .fetch_all(conn)
    .await?;
    // SQLite does not define the order of RETURNING rows
    works.sort_by_key(|work| work.id);
    Ok(works
        .into_iter()
        .map(|work| {
            audit_subject(
                WorkId::new(work.id),
                LibraryId::new(work.library_id),
                &work.title,
            )
        })
        .collect())
}

/// Delete every work in `library` that no publication contains
///
/// Return the audit subject of each work deleted, in id order.
pub(crate) async fn collect_orphan_works(
    conn: &mut SqliteConnection,
    library: LibraryId,
) -> Result<Vec<AuditSubject>> {
    let mut works = sqlx::query!(
        "DELETE FROM work
         WHERE library_id = ? AND id NOT IN (SELECT work_id FROM publication_work)
         RETURNING id AS \"id!\", title",
        library.get()
    )
    .fetch_all(conn)
    .await?;
    // SQLite does not define the order of RETURNING rows
    works.sort_by_key(|work| work.id);
    Ok(works
        .into_iter()
        .map(|work| audit_subject(WorkId::new(work.id), library, &work.title))
        .collect())
}

/// List every work in `library`
pub(crate) async fn in_library(engine: &Engine, library: LibraryId) -> Result<Vec<Work>> {
    let mut conn = engine.begin_read().await?;
    let library = library.get();
    let rows = sqlx::query_as!(
        WorkRow,
        r#"SELECT id, library_id, title, key, time_signature, instrumentation,
                  stars AS "stars: u8", note, kind, draft AS "draft: bool",
                  created_at AS "created_at: Timestamp",
                  last_modified_at AS "last_modified_at: Timestamp"
           FROM work WHERE library_id = ? ORDER BY id"#,
        library
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut works = Vec::with_capacity(rows.len());
    for row in rows {
        works.push(Work::from_row(engine, &mut conn, row).await?);
    }
    Ok(works)
}

/// List every work in `publication`
pub(crate) async fn in_publication(
    engine: &Engine,
    publication: PublicationId,
) -> Result<Vec<Work>> {
    let mut conn = engine.begin_read().await?;
    let publication = publication.get();
    let rows = sqlx::query_as!(
        WorkRow,
        r#"SELECT work.id, work.library_id, work.title, work.key, work.time_signature,
                  work.instrumentation, work.stars AS "stars: u8", work.note, work.kind,
                  work.draft AS "draft: bool",
                  work.created_at AS "created_at: Timestamp",
                  work.last_modified_at AS "last_modified_at: Timestamp"
           FROM work JOIN publication_work ON publication_work.work_id = work.id
           WHERE publication_work.publication_id = ?
           ORDER BY work.id"#,
        publication
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut works = Vec::with_capacity(rows.len());
    for row in rows {
        works.push(Work::from_row(engine, &mut conn, row).await?);
    }
    Ok(works)
}

/// List every work that names `person` as a contributor
pub(crate) async fn with_contributor(engine: &Engine, person: PersonId) -> Result<Vec<Work>> {
    let mut conn = engine.begin_read().await?;
    let person = person.get();
    // A person can contribute in several roles, so a join would repeat the work
    let rows = sqlx::query_as!(
        WorkRow,
        r#"SELECT id, library_id, title, key, time_signature, instrumentation,
                  stars AS "stars: u8", note, kind, draft AS "draft: bool",
                  created_at AS "created_at: Timestamp",
                  last_modified_at AS "last_modified_at: Timestamp"
           FROM work
           WHERE id IN (SELECT work_id FROM work_contributor WHERE person_id = ?)
           ORDER BY id"#,
        person
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut works = Vec::with_capacity(rows.len());
    for row in rows {
        works.push(Work::from_row(engine, &mut conn, row).await?);
    }
    Ok(works)
}

fn audit_subject(id: WorkId, library: LibraryId, title: &str) -> AuditSubject {
    AuditSubject::Entity {
        kind: EntityKind::Work,
        id: id.get(),
        library,
        label: title.to_string(),
    }
}

pub(crate) fn clean_title(title: &str) -> Result<&str> {
    let title = title.trim();
    if title.is_empty() {
        return Err(Error::InvalidValue("A work title cannot be blank.".into()));
    }
    Ok(title)
}

/// A catalog numbering scheme that scorarium knows how to parse
struct Scheme {
    aliases: &'static [&'static str],
}

/// Listed in priority order: earlier schemes are parsed with higher confidence.
static SCHEMES: &[Scheme] = &[
    // Opus number, the "standard" catalog number scheme.
    //
    // https://en.wikipedia.org/wiki/Opus_number
    Scheme {
        aliases: &["op", "opus"],
    },
    // Bach-Werke-Verzeichnis, Wolfgang Schmieder's catalogue of J. S. Bach
    //
    // https://en.wikipedia.org/wiki/Bach-Werke-Verzeichnis
    Scheme { aliases: &["bwv"] },
    // Koechel catalogue of Mozart, written K. or KV (Koechel-Verzeichnis)
    //
    // https://en.wikipedia.org/wiki/K%C3%B6chel_catalogue
    Scheme {
        aliases: &["k", "kv"],
    },
    // Deutsch catalogue of Schubert
    //
    // https://en.wikipedia.org/wiki/Deutsch_catalogue
    Scheme { aliases: &["d"] },
    // Handel-Werke-Verzeichnis, Bernd Baselt's catalogue of Handel
    //
    // https://en.wikipedia.org/wiki/H%C3%A4ndel-Werke-Verzeichnis
    Scheme { aliases: &["hwv"] },
    // Ryom-Verzeichnis, Peter Ryom's catalogue of Vivaldi
    //
    // https://en.wikipedia.org/wiki/Ryom-Verzeichnis
    Scheme { aliases: &["rv"] },
    // Werk ohne Opuszahl, a work its composer published without an opus number, chiefly Beethoven
    //
    // https://en.wikipedia.org/wiki/WoO
    Scheme { aliases: &["woo"] },
    // Humphrey Searle's catalogue of Liszt
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_Franz_Liszt
    Scheme { aliases: &["s"] },
    // Andras Szollosy's catalogue of Bartok
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_B%C3%A9la_Bart%C3%B3k
    Scheme { aliases: &["sz"] },
    // Laszlo Somfai's catalogue of Bartok
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_B%C3%A9la_Bart%C3%B3k
    Scheme { aliases: &["bb"] },
    // Jarmil Burghauser's catalogue of Dvorak
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_Anton%C3%ADn_Dvo%C5%99%C3%A1k
    Scheme { aliases: &["b"] },
    // Francois Lesure's catalogue of Debussy
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_Claude_Debussy
    Scheme { aliases: &["l"] },
    // Franklin Zimmerman's catalogue of Purcell
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_Henry_Purcell
    Scheme { aliases: &["z"] },
    // Wydanie Narodowe, Jan Ekier's Polish National Edition of Chopin
    //
    // https://en.wikipedia.org/wiki/List_of_compositions_by_Fr%C3%A9d%C3%A9ric_Chopin
    Scheme { aliases: &["wn"] },
    // Ralph Kirkpatrick's catalogue of Scarlatti's keyboard sonatas, written Kk. to keep it apart
    // from Mozart's K. Chopin's Kobylanska catalogue also uses KK, but numbers such as
    // "KK IVa/16" carry a Roman numeral, so they stay unrecognized rather than parsing as this.
    //
    // https://en.wikipedia.org/wiki/List_of_solo_keyboard_sonatas_by_Domenico_Scarlatti
    Scheme { aliases: &["kk"] },
];

/// A catalog number
///
/// Two values are equal when a musician would read them as the same number, so equal values can
/// print differently: "Op. 27/2" equals "Op. 27 No. 2".
#[derive(Clone, Debug)]
pub struct CatalogNumber {
    /// Raw text entered from the user
    text: String,
    /// Index into [SCHEMES]; None when unrecognized
    scheme: Option<usize>,
    /// The sort tuple: the parsed numbers, or every run of digits when unrecognized
    numbers: Vec<u32>,
}

impl CatalogNumber {
    /// The raw text entered by the user
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether the parser knows the scheme this number is written in
    pub fn is_recognized(&self) -> bool {
        self.scheme.is_some()
    }

    /// Get an [Ord]-safe sort key for [CatalogNumber]
    ///
    /// Recognized number schemes are sorted first, in order of decreasing confidence. Unrecognized
    /// numbers sort by the strings of digits in them like a tuple, and then alphabetically to break
    /// ties.
    fn key(&self) -> (usize, bool, &[u32], Option<&str>) {
        (
            self.scheme.unwrap_or(usize::MAX),
            self.numbers.is_empty(),
            &self.numbers,
            self.scheme.is_none().then_some(self.text.as_str()),
        )
    }
}

impl FromStr for CatalogNumber {
    type Err = Error;

    /// Any non-empty catalog number is accepted because there's no universal numbering scheme or
    /// parser that can handle every edge case, of which there are many.
    fn from_str(input: &str) -> Result<CatalogNumber> {
        let text = input.trim();
        if text.is_empty() {
            return Err(Error::InvalidValue(
                "A catalog number cannot be empty.".to_string(),
            ));
        }
        let (scheme, numbers) = match recognize(text) {
            Some((scheme, numbers)) => (Some(scheme), numbers),
            None => (None, digit_runs(text)),
        };
        Ok(CatalogNumber {
            text: text.to_string(),
            scheme,
            numbers,
        })
    }
}

impl fmt::Display for CatalogNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl PartialEq for CatalogNumber {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl Eq for CatalogNumber {}

impl Ord for CatalogNumber {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

impl PartialOrd for CatalogNumber {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn recognize(text: &str) -> Option<(usize, Vec<u32>)> {
    let (label, rest) = split_letters(text);
    let scheme = SCHEMES.iter().position(|scheme| {
        scheme
            .aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(label))
    })?;
    let rest = rest.strip_prefix('.').unwrap_or(rest).trim_start();
    let (number, rest) = integer(rest)?;
    let rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
    if rest.is_empty() {
        return Some((scheme, vec![number]));
    }
    let rest = match rest.strip_prefix('/') {
        Some(rest) => rest,
        None => sub_number_word(rest)?,
    };
    let (sub_number, rest) = integer(rest.trim_start())?;
    rest.is_empty().then(|| (scheme, vec![number, sub_number]))
}

fn split_letters(text: &str) -> (&str, &str) {
    let end = text
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(text.len());
    text.split_at(end)
}

fn integer(text: &str) -> Option<(u32, &str)> {
    let end = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let number = text[..end].parse().ok()?;
    Some((number, &text[end..]))
}

/// "No. 2", "no 2", "Nr. 2", "N. 2", "Number 2": the word with an optional dot, returning what follows
fn sub_number_word(text: &str) -> Option<&str> {
    let (word, rest) = split_letters(text);
    ["no", "nr", "n", "number"]
        .iter()
        .any(|known| known.eq_ignore_ascii_case(word))
        .then(|| rest.strip_prefix('.').unwrap_or(rest))
}

/// Every run of digits, in order; a run too long for u32 is skipped
fn digit_runs(text: &str) -> Vec<u32> {
    text.split(|c: char| !c.is_ascii_digit())
        .filter(|run| !run.is_empty())
        .filter_map(|run| run.parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(text: &str) -> CatalogNumber {
        text.parse().unwrap()
    }

    #[test]
    fn every_spelling_of_an_opus_number_is_equal() {
        let canonical = number("Op. 27 No. 2");
        for spelling in [
            "Op. 27, No. 2",
            "Op. 27/2",
            "Op 27 Nr. 2",
            "op.27 no.2",
            "Opus 27 Number 2",
            "Op 27 No 2",
            "  Op. 27 No. 2  ",
        ] {
            let n = number(spelling);
            assert!(n.is_recognized(), "{spelling:?}");
            assert_eq!(n, canonical, "{spelling:?}");
        }
        assert_ne!(number("Op. 27"), canonical);
        assert_ne!(number("Op. 27 No. 3"), canonical);
        assert_ne!(number("BWV 27"), number("Op. 27"));
        // Text is kept as typed, apart from trimming
        assert_eq!(number("  op.27/2 ").as_str(), "op.27/2");
    }

    #[test]
    fn composer_catalogs_parse_to_their_scheme_and_numbers() {
        let opus = number("Op. 1").scheme;
        for (text, numbers) in [
            ("BWV 988", vec![988]),
            ("K. 331", vec![331]),
            ("KV 331", vec![331]),
            ("D 960", vec![960]),
            ("D. 899 No. 3", vec![899, 3]),
            ("WoO 59", vec![59]),
            ("S. 244/2", vec![244, 2]),
            ("Sz. 107/97", vec![107, 97]),
            ("BB 105", vec![105]),
            ("HWV 56", vec![56]),
            ("RV 269", vec![269]),
            ("Z 626", vec![626]),
            ("L. 75", vec![75]),
            ("B. 178", vec![178]),
            ("WN 37", vec![37]),
            ("Kk. 380", vec![380]),
        ] {
            let n = number(text);
            assert!(n.scheme.is_some(), "{text:?} should be recognized");
            assert_ne!(n.scheme, opus, "{text:?} is not an opus number");
            assert_eq!(n.numbers, numbers, "{text:?}");
        }
        // Same letters, different catalogs: K. is Koechel, Kk. is Kirkpatrick
        assert_ne!(number("K. 380"), number("Kk. 380"));
    }

    #[test]
    fn awkward_numbers_are_unrecognized_but_keep_their_digits() {
        for (text, digits) in [
            ("Op. posth. 66", vec![66]),
            ("Op. 72 No. 1 (Posthumous opus)", vec![72, 1]),
            ("Op. 37a No. 6", vec![37, 6]),
            ("Op. 19b", vec![19]),
            ("BWV Anh. 114", vec![114]),
            ("BWV 846-869", vec![846, 869]),
            ("Hob. XVI:52", vec![52]),
            ("K. 331/300i", vec![331, 300]),
            ("KK IVa/16", vec![16]),
            ("MWV O 14", vec![14]),
            ("No. 1", vec![1]),
            ("none", vec![]),
        ] {
            let n = number(text);
            assert!(!n.is_recognized(), "{text:?}");
            assert_eq!(n.numbers, digits, "{text:?}");
            assert_eq!(n.as_str(), text.trim(), "{text:?}");
        }
        // Unrecognized values are equal only on identical text
        assert_eq!(number("Hob. XVI:52"), number("Hob. XVI:52"));
        assert_ne!(number("Hob. XVI:52"), number("Hob XVI:52"));
        assert!(matches!(
            "  ".parse::<CatalogNumber>(),
            Err(Error::InvalidValue(_))
        ));
    }

    #[test]
    fn sorts_by_scheme_then_tuple_with_unrecognized_last() {
        let mut numbers: Vec<CatalogNumber> = [
            "Op. posth. 66",
            "D 960",
            "Op. 100",
            "BWV 988",
            "Op. 27 No. 2",
            "Hob. XVI:52",
            "Op. 16",
            "Op. 27",
            "Op. 87 No. 24",
            "BWV 846",
            "none",
        ]
        .into_iter()
        .map(number)
        .collect();
        numbers.sort();
        let sorted: Vec<&str> = numbers.iter().map(CatalogNumber::as_str).collect();
        assert_eq!(
            sorted,
            [
                "Op. 16",
                "Op. 27",
                "Op. 27 No. 2",
                "Op. 87 No. 24",
                "Op. 100",
                "BWV 846",
                "BWV 988",
                "D 960",
                // Unrecognized: by digit runs, then text; no digits at all sorts last
                "Hob. XVI:52",
                "Op. posth. 66",
                "none",
            ]
        );
    }
}
