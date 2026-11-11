use std::fmt;
use std::num::ParseIntError;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;

use sqlx::{Connection, Sqlite, SqliteConnection, Transaction};

use crate::entity::{AnyEntityId, EntityKind, Timestamp};
use crate::library::LibraryId;
use crate::person::PersonId;
use crate::publication::PublicationId;
use crate::work::WorkId;
use crate::{Engine, Error, Result};

const MAX_EVENTS: usize = 3_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AuditId(i64);

impl AuditId {
    pub(crate) fn new(id: i64) -> AuditId {
        AuditId(id)
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

impl fmt::Display for AuditId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for AuditId {
    type Err = ParseIntError;

    fn from_str(text: &str) -> std::result::Result<AuditId, ParseIntError> {
        text.parse().map(AuditId)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditSource {
    User,
    OrphanCleanup,
    Merge,
    LinkRecognition,
    Enrichment,
}

impl fmt::Display for AuditSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AuditSource::User => "user",
            AuditSource::OrphanCleanup => "orphan_cleanup",
            AuditSource::Merge => "merge",
            AuditSource::LinkRecognition => "link_recognition",
            AuditSource::Enrichment => "enrichment",
        })
    }
}

impl FromStr for AuditSource {
    type Err = Error;

    fn from_str(text: &str) -> Result<AuditSource> {
        Ok(match text {
            "user" => AuditSource::User,
            "orphan_cleanup" => AuditSource::OrphanCleanup,
            "merge" => AuditSource::Merge,
            "link_recognition" => AuditSource::LinkRecognition,
            "enrichment" => AuditSource::Enrichment,
            other => return Err(eyre::eyre!("unknown audit source {other:?}").into()),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Created,
    Updated,
    Deleted,
    Merged,
    Committed,
    PasswordClaimed,
    PasswordChanged,
    PasswordReset,
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Action::Created => "created",
            Action::Updated => "updated",
            Action::Deleted => "deleted",
            Action::Merged => "merged",
            Action::Committed => "committed",
            Action::PasswordClaimed => "password_claimed",
            Action::PasswordChanged => "password_changed",
            Action::PasswordReset => "password_reset",
        })
    }
}

impl FromStr for Action {
    type Err = Error;

    fn from_str(text: &str) -> Result<Action> {
        Ok(match text {
            "created" => Action::Created,
            "updated" => Action::Updated,
            "deleted" => Action::Deleted,
            "merged" => Action::Merged,
            "committed" => Action::Committed,
            "password_claimed" => Action::PasswordClaimed,
            "password_changed" => Action::PasswordChanged,
            "password_reset" => Action::PasswordReset,
            other => return Err(eyre::eyre!("unknown audit action {other:?}").into()),
        })
    }
}

/// The object an [AuditEvent] refers to
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuditSubject {
    Library {
        id: LibraryId,
        label: String,
    },
    Entity {
        id: AnyEntityId,
        library: LibraryId,
        label: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditEvent {
    pub id: AuditId,
    pub created_at: Timestamp,
    pub source: AuditSource,
    pub action: Action,
    pub subject: Option<AuditSubject>,
    pub subject_exists: bool,
    pub field: Option<String>,
}

pub(crate) struct Audited<'a> {
    tx: Transaction<'a, Sqlite>,
}

impl<'a> Audited<'a> {
    pub(crate) fn new(tx: Transaction<'a, Sqlite>) -> Audited<'a> {
        Audited { tx }
    }

    /// Record the audit log event.
    ///
    /// Returns a [Recorded] typestate that allows committing the transaction.
    pub(crate) async fn record(
        mut self,
        source: AuditSource,
        action: Action,
        subject: Option<&AuditSubject>,
        field: Option<&str>,
    ) -> Result<Recorded<'a>> {
        insert(&mut self.tx, source, action, subject, field).await?;
        Ok(Recorded {
            tx: self.tx,
            pending: vec![describe(source, action, subject, field)],
        })
    }

    /// Commit the transaction without recording an audit event.
    #[deprecated(note = "Use Audited::record()")]
    pub(crate) async fn commit_unaudited(self) -> Result<()> {
        self.tx.commit().await?;
        Ok(())
    }
}

pub(crate) struct Recorded<'a> {
    tx: Transaction<'a, Sqlite>,
    /// The log lines to emit once the transaction commits.
    pending: Vec<String>,
}

impl<'a> Recorded<'a> {
    pub(crate) async fn record(
        mut self,
        source: AuditSource,
        action: Action,
        subject: Option<&AuditSubject>,
        field: Option<&str>,
    ) -> Result<Recorded<'a>> {
        insert(&mut self.tx, source, action, subject, field).await?;
        self.pending.push(describe(source, action, subject, field));
        Ok(self)
    }

    /// Record one audit event per subject, in order
    pub(crate) async fn record_each(
        mut self,
        source: AuditSource,
        action: Action,
        subjects: &[AuditSubject],
        field: Option<&str>,
    ) -> Result<Recorded<'a>> {
        for subject in subjects {
            insert(&mut self.tx, source, action, Some(subject), field).await?;
            self.pending
                .push(describe(source, action, Some(subject), field));
        }
        Ok(self)
    }

    pub(crate) async fn commit(mut self) -> Result<()> {
        let keep = MAX_EVENTS as i64;
        sqlx::query!(
            "DELETE FROM audit_entry
             WHERE id NOT IN (SELECT id FROM audit_entry ORDER BY id DESC LIMIT ?)",
            keep
        )
        .execute(&mut *self.tx)
        .await?;
        self.tx.commit().await?;
        for line in &self.pending {
            tracing::info!("{line}");
        }
        Ok(())
    }
}

impl Deref for Audited<'_> {
    type Target = SqliteConnection;

    fn deref(&self) -> &SqliteConnection {
        &self.tx
    }
}

impl DerefMut for Audited<'_> {
    fn deref_mut(&mut self) -> &mut SqliteConnection {
        &mut self.tx
    }
}

impl Deref for Recorded<'_> {
    type Target = SqliteConnection;

    fn deref(&self) -> &SqliteConnection {
        &self.tx
    }
}

impl DerefMut for Recorded<'_> {
    fn deref_mut(&mut self) -> &mut SqliteConnection {
        &mut self.tx
    }
}

/// Describe an audit event in one line, for the log
fn describe(
    source: AuditSource,
    action: Action,
    subject: Option<&AuditSubject>,
    field: Option<&str>,
) -> String {
    let mut line = format!("{source} {action}");
    match subject {
        None => {}
        Some(AuditSubject::Library { id, label }) => {
            line.push_str(&format!(" library {id} {label:?}"));
        }
        Some(AuditSubject::Entity { id, label, .. }) => {
            line.push_str(&format!(" {} {} {label:?}", id.kind(), id.get()));
        }
    }
    if let Some(field) = field {
        line.push(' ');
        line.push_str(field);
    }
    line
}

async fn insert(
    tx: &mut SqliteConnection,
    source: AuditSource,
    action: Action,
    subject: Option<&AuditSubject>,
    field: Option<&str>,
) -> Result<()> {
    let (kind, id, library, label) = match subject {
        None => (None, None, None, None),
        Some(AuditSubject::Library { id, label }) => (
            Some("library".to_string()),
            Some(id.get()),
            None,
            Some(label),
        ),
        Some(AuditSubject::Entity { id, library, label }) => (
            Some(id.kind().to_string()),
            Some(id.get()),
            Some(library.get()),
            Some(label),
        ),
    };
    let source = source.to_string();
    let action = action.to_string();
    sqlx::query!(
        "INSERT INTO audit_entry
             (source, action, entity_kind, entity_id, entity_library_id, entity_label, field)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        source,
        action,
        kind,
        id,
        library,
        label,
        field
    )
    .execute(&mut *tx)
    .await?;
    Ok(())
}

struct AuditRow {
    id: i64,
    created_at: Timestamp,
    source: String,
    action: String,
    entity_kind: Option<String>,
    entity_id: Option<i64>,
    entity_library_id: Option<i64>,
    entity_label: Option<String>,
    field: Option<String>,
    subject_exists: bool,
}

impl AuditRow {
    fn into_event(self) -> Result<AuditEvent> {
        let missing = |column| eyre::eyre!("audit event {} has no {column}", self.id);
        let label = self.entity_label.unwrap_or_default();
        let subject = match self.entity_kind.as_deref() {
            None => None,
            Some("library") => Some(AuditSubject::Library {
                id: LibraryId::new(self.entity_id.ok_or_else(|| missing("entity_id"))?),
                label,
            }),
            Some(kind) => Some(AuditSubject::Entity {
                id: {
                    let id = self.entity_id.ok_or_else(|| missing("entity_id"))?;
                    match kind.parse()? {
                        EntityKind::Publication => AnyEntityId::Publication(PublicationId::new(id)),
                        EntityKind::Work => AnyEntityId::Work(WorkId::new(id)),
                        EntityKind::Person => AnyEntityId::Person(PersonId::new(id)),
                    }
                },
                library: LibraryId::new(
                    self.entity_library_id
                        .ok_or_else(|| missing("entity_library_id"))?,
                ),
                label,
            }),
        };
        Ok(AuditEvent {
            id: AuditId::new(self.id),
            created_at: self.created_at,
            source: self.source.parse()?,
            action: self.action.parse()?,
            subject,
            subject_exists: self.subject_exists,
            field: self.field,
        })
    }
}

fn size_to_id(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

/// One page of the audit log
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditPage {
    /// Newest first
    pub events: Vec<AuditEvent>,
    /// Events in the whole log, or on the entity
    pub total: usize,
}

impl Engine {
    /// Read a page of the audit log, newest first, starting `offset` events in
    pub async fn audit_log(&self, offset: usize, limit: usize) -> Result<AuditPage> {
        let offset = size_to_id(offset);
        let limit = size_to_id(limit);
        let mut conn = self.begin_read().await?;
        // One transaction so that the total and the page see the same snapshot.
        let mut tx = conn.begin().await?;
        let total = sqlx::query_scalar!("SELECT COUNT(*) FROM audit_entry")
            .fetch_one(&mut *tx)
            .await?;
        let rows = sqlx::query_as!(
            AuditRow,
            r#"SELECT audit_entry.id, audit_entry.created_at AS "created_at: Timestamp",
                      source, action, entity_kind, entity_id, entity_library_id, entity_label,
                      field,
                      (library.id IS NOT NULL OR publication.id IS NOT NULL
                       OR work.id IS NOT NULL OR person.id IS NOT NULL) AS "subject_exists!: bool"
               FROM audit_entry
               LEFT JOIN library ON entity_kind = 'library' AND library.id = entity_id
               LEFT JOIN publication ON entity_kind = 'publication'
                   AND publication.id = entity_id AND publication.library_id = entity_library_id
               LEFT JOIN work ON entity_kind = 'work'
                   AND work.id = entity_id AND work.library_id = entity_library_id
               LEFT JOIN person ON entity_kind = 'person'
                   AND person.id = entity_id AND person.library_id = entity_library_id
               ORDER BY audit_entry.id DESC
               LIMIT ?2 OFFSET ?1"#,
            offset,
            limit
        )
        .fetch_all(&mut *tx)
        .await?;
        Ok(AuditPage {
            events: rows
                .into_iter()
                .map(AuditRow::into_event)
                .collect::<Result<_>>()?,
            total: usize::try_from(total).map_err(eyre::Report::from)?,
        })
    }

    /// Read a page of an entity's audit log, as [Engine::audit_log]
    pub(crate) async fn entity_audit_log(
        &self,
        id: AnyEntityId,
        library: LibraryId,
        offset: usize,
        limit: usize,
    ) -> Result<AuditPage> {
        let kind = id.kind().to_string();
        let id = id.get();
        let library = library.get();
        let offset = size_to_id(offset);
        let limit = size_to_id(limit);
        let mut conn = self.begin_read().await?;
        let mut tx = conn.begin().await?;
        let total = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM audit_entry
             WHERE entity_kind = ?1 AND entity_id = ?2 AND entity_library_id = ?3",
            kind,
            id,
            library
        )
        .fetch_one(&mut *tx)
        .await?;
        let rows = sqlx::query_as!(
            AuditRow,
            r#"SELECT audit_entry.id, audit_entry.created_at AS "created_at: Timestamp",
                      source, action, entity_kind, entity_id, entity_library_id, entity_label,
                      field,
                      (library.id IS NOT NULL OR publication.id IS NOT NULL
                       OR work.id IS NOT NULL OR person.id IS NOT NULL) AS "subject_exists!: bool"
               FROM audit_entry
               LEFT JOIN library ON entity_kind = 'library' AND library.id = entity_id
               LEFT JOIN publication ON entity_kind = 'publication'
                   AND publication.id = entity_id AND publication.library_id = entity_library_id
               LEFT JOIN work ON entity_kind = 'work'
                   AND work.id = entity_id AND work.library_id = entity_library_id
               LEFT JOIN person ON entity_kind = 'person'
                   AND person.id = entity_id AND person.library_id = entity_library_id
               WHERE entity_kind = ?1 AND entity_id = ?2 AND entity_library_id = ?3
               ORDER BY audit_entry.id DESC
               LIMIT ?5 OFFSET ?4"#,
            kind,
            id,
            library,
            offset,
            limit
        )
        .fetch_all(&mut *tx)
        .await?;
        Ok(AuditPage {
            events: rows
                .into_iter()
                .map(AuditRow::into_event)
                .collect::<Result<_>>()?,
            total: usize::try_from(total).map_err(eyre::Report::from)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::Entity;
    use crate::holding::NewHolding;
    use crate::publication::ImportSeed;

    #[tokio::test]
    async fn audit_events_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        let library = AuditSubject::Library {
            id: LibraryId::new(1),
            label: "Scores".into(),
        };
        let work = AuditSubject::Entity {
            id: AnyEntityId::Work(WorkId::new(7)),
            library: LibraryId::new(1),
            label: "Sonata".into(),
        };
        let tx = engine.begin_write().await.unwrap();
        let tx = tx
            .record(AuditSource::User, Action::Created, Some(&library), None)
            .await
            .unwrap();
        let tx = tx
            .record(
                AuditSource::Merge,
                Action::Updated,
                Some(&work),
                Some("title"),
            )
            .await
            .unwrap();
        let tx = tx
            .record(AuditSource::User, Action::PasswordClaimed, None, None)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let events = engine.audit_log(0, 10).await.unwrap().events;
        let subjects: Vec<_> = events
            .iter()
            .map(|e| (e.action, e.subject.clone(), e.field.clone()))
            .collect();
        assert_eq!(
            subjects,
            [
                (Action::PasswordClaimed, None, None),
                (Action::Updated, Some(work), Some("title".to_string())),
                (Action::Created, Some(library), None),
            ]
        );

        let page = engine.audit_log(1, 10).await.unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(
            page.events.iter().map(|e| e.action).collect::<Vec<_>>(),
            [Action::Updated, Action::Created]
        );
    }

    #[tokio::test]
    async fn subject_exists_follows_the_subject() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        let library = engine
            .create_library("Scores".parse().unwrap(), false)
            .await
            .unwrap();
        let mut publication = library
            .import(
                ImportSeed::Title {
                    title: "Gymnopedies".into(),
                    record: None,
                },
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        publication.commit().await.unwrap();
        let subjects_exist = async || {
            let events = engine.audit_log(0, 100).await.unwrap().events;
            let exist = |library: bool| {
                events
                    .iter()
                    .filter(|e| matches!(e.subject, Some(AuditSubject::Library { .. })) == library)
                    .map(|e| e.subject_exists)
                    .collect::<Vec<_>>()
            };
            (exist(true), exist(false))
        };

        let (library_events, publication_events) = subjects_exist().await;
        assert!(library_events.iter().all(|&exists| exists));
        assert!(!publication_events.is_empty() && publication_events.iter().all(|&exists| exists));

        publication.delete().await.unwrap();
        let (library_events, publication_events) = subjects_exist().await;
        assert!(library_events.iter().all(|&exists| exists));
        assert!(publication_events.iter().all(|&exists| !exists));
    }
}
