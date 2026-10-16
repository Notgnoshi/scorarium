use std::ops::{Deref, DerefMut};

use sqlx::{Sqlite, SqliteConnection, Transaction};

const MAX_ENTRIES: usize = 3_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// An event the user initiated
    User,
    /// An event triggered by doing orphan cleanup
    OrphanCleanup,
    /// An event triggered by merging two entities
    Merge,
    /// An event triggered by recomputing which records links name when the archive opens
    LinkRecognition,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::User => "user",
            Source::OrphanCleanup => "orphan_cleanup",
            Source::Merge => "merge",
            Source::LinkRecognition => "link_recognition",
        }
    }

    fn parse(text: &str) -> crate::Result<Source> {
        Ok(match text {
            "user" => Source::User,
            "orphan_cleanup" => Source::OrphanCleanup,
            "merge" => Source::Merge,
            "link_recognition" => Source::LinkRecognition,
            other => eyre::bail!("unknown audit source {other:?}"),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Created,
    Updated,
    Deleted,
    Merged,
    ImportAccepted,
    PasswordClaimed,
    PasswordChanged,
}

impl Action {
    pub fn as_str(self) -> &'static str {
        match self {
            Action::Created => "created",
            Action::Updated => "updated",
            Action::Deleted => "deleted",
            Action::Merged => "merged",
            Action::ImportAccepted => "import_accepted",
            Action::PasswordClaimed => "password_claimed",
            Action::PasswordChanged => "password_changed",
        }
    }

    fn parse(text: &str) -> crate::Result<Action> {
        Ok(match text {
            "created" => Action::Created,
            "updated" => Action::Updated,
            "deleted" => Action::Deleted,
            "merged" => Action::Merged,
            "import_accepted" => Action::ImportAccepted,
            "password_claimed" => Action::PasswordClaimed,
            "password_changed" => Action::PasswordChanged,
            other => eyre::bail!("unknown audit action {other:?}"),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AuditSubject {
    Publication,
    Work,
    Person,
    Library,
}

impl AuditSubject {
    pub fn as_str(self) -> &'static str {
        match self {
            AuditSubject::Publication => "publication",
            AuditSubject::Work => "work",
            AuditSubject::Person => "person",
            AuditSubject::Library => "library",
        }
    }

    fn parse(text: &str) -> crate::Result<AuditSubject> {
        Ok(match text {
            "publication" => AuditSubject::Publication,
            "work" => AuditSubject::Work,
            "person" => AuditSubject::Person,
            "library" => AuditSubject::Library,
            other => eyre::bail!("unknown audit entity kind {other:?}"),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Title,
    Publisher,
    Year,
    Identifiers,
    Contributors,
    Holdings,
    Contents,
    Key,
    TimeSignature,
    Instrumentation,
    CatalogNumbers,
    Name,
    Visibility,
    Stars,
    Note,
    Tags,
    Links,
}

impl Field {
    pub fn as_str(self) -> &'static str {
        match self {
            Field::Title => "title",
            Field::Publisher => "publisher",
            Field::Year => "year",
            Field::Identifiers => "identifiers",
            Field::Contributors => "contributors",
            Field::Holdings => "holdings",
            Field::Contents => "contents",
            Field::Key => "key",
            Field::TimeSignature => "time_signature",
            Field::Instrumentation => "instrumentation",
            Field::CatalogNumbers => "catalog_numbers",
            Field::Name => "name",
            Field::Visibility => "visibility",
            Field::Stars => "stars",
            Field::Note => "note",
            Field::Tags => "tags",
            Field::Links => "links",
        }
    }

    fn parse(text: &str) -> crate::Result<Field> {
        Ok(match text {
            "title" => Field::Title,
            "publisher" => Field::Publisher,
            "year" => Field::Year,
            "identifiers" => Field::Identifiers,
            "contributors" => Field::Contributors,
            "holdings" => Field::Holdings,
            "contents" => Field::Contents,
            "key" => Field::Key,
            "time_signature" => Field::TimeSignature,
            "instrumentation" => Field::Instrumentation,
            "catalog_numbers" => Field::CatalogNumbers,
            "name" => Field::Name,
            "visibility" => Field::Visibility,
            "stars" => Field::Stars,
            "note" => Field::Note,
            "tags" => Field::Tags,
            "links" => Field::Links,
            other => eyre::bail!("unknown audit field {other:?}"),
        })
    }
}

/// What an entry points at, when it points at anything
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityRef {
    pub kind: AuditSubject,
    pub id: i64,
    pub library_id: Option<i64>,
    /// The entity's name as it read when the entry was written, so a deleted entity still renders
    pub label: String,
}

/// One row of the audit log
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditEntry {
    pub id: i64,
    pub created_at: i64,
    pub source: Source,
    pub action: Action,
    pub entity: Option<EntityRef>,
    pub field: Option<Field>,
}

/// A write transaction that records what it did before it commits.
pub(crate) struct Audited<'a> {
    tx: Transaction<'a, Sqlite>,
}

impl<'a> Audited<'a> {
    pub(crate) fn new(tx: Transaction<'a, Sqlite>) -> Audited<'a> {
        Audited { tx }
    }

    /// Record one action, once the mutation has produced the entity and the diff it names
    pub(crate) async fn record(
        &mut self,
        source: Source,
        action: Action,
        entity: Option<EntityRef>,
        field: Option<Field>,
    ) -> crate::Result<()> {
        let source = source.as_str();
        let action = action.as_str();
        let kind = entity.as_ref().map(|e| e.kind.as_str());
        let id = entity.as_ref().map(|e| e.id);
        let library_id = entity.as_ref().and_then(|e| e.library_id);
        let label = entity.as_ref().map(|e| e.label.as_str());
        let field = field.map(|field| field.as_str());
        sqlx::query!(
            "INSERT INTO audit_entry
                 (source, action, entity_kind, entity_id, entity_library_id, entity_label, field)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            source,
            action,
            kind,
            id,
            library_id,
            label,
            field
        )
        .execute(&mut *self.tx)
        .await?;
        Ok(())
    }

    pub(crate) async fn commit(mut self) -> crate::Result<()> {
        trim(&mut self.tx).await?;
        self.tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn rollback(self) -> crate::Result<()> {
        self.tx.rollback().await?;
        Ok(())
    }
}

// So call sites keep the `&mut *tx` idiom they already use
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

/// Keep the newest [MAX_ENTRIES] entries
async fn trim(conn: &mut SqliteConnection) -> crate::Result<()> {
    let keep = MAX_ENTRIES as i64;
    sqlx::query!(
        "DELETE FROM audit_entry
         WHERE id NOT IN (SELECT id FROM audit_entry ORDER BY id DESC LIMIT ?)",
        keep
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Read entries newest first, below `before` when given, at most `limit` when given.
///
/// The cursor is a keyset rather than an offset because an offset shifts under a reader paging
/// through while writes land.
pub(crate) async fn load_entries(
    conn: &mut SqliteConnection,
    before: Option<i64>,
    limit: Option<usize>,
) -> crate::Result<Vec<AuditEntry>> {
    // SQLite reads a negative LIMIT as no limit
    let limit = limit.map_or(-1, |limit| limit as i64);
    let rows = sqlx::query!(
        r#"SELECT id, created_at, source, action,
                  entity_kind, entity_id, entity_library_id, entity_label, field
           FROM audit_entry
           WHERE ?1 IS NULL OR id < ?1
           ORDER BY id DESC
           LIMIT ?2"#,
        before,
        limit
    )
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter()
        .map(|row| {
            let entity = match row.entity_kind {
                None => None,
                Some(kind) => Some(EntityRef {
                    kind: AuditSubject::parse(&kind)?,
                    id: row
                        .entity_id
                        .ok_or_else(|| eyre::eyre!("audit entry {} names no entity id", row.id))?,
                    library_id: row.entity_library_id,
                    label: row.entity_label.unwrap_or_default(),
                }),
            };
            Ok(AuditEntry {
                id: row.id,
                created_at: row.created_at,
                source: Source::parse(&row.source)?,
                action: Action::parse(&row.action)?,
                entity,
                field: row.field.as_deref().map(Field::parse).transpose()?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Archive;

    fn publication(label: &str) -> Option<EntityRef> {
        Some(EntityRef {
            kind: AuditSubject::Publication,
            id: 1,
            library_id: Some(1),
            label: label.to_string(),
        })
    }

    #[tokio::test]
    async fn trimming_keeps_the_newest_entries() {
        let archive = Archive::in_memory().await.unwrap();
        let mut audited = archive.shared().begin_write().await.unwrap();
        for n in 0..(MAX_ENTRIES + 3) {
            audited
                .record(
                    Source::User,
                    Action::Updated,
                    publication(&format!("p{n}")),
                    None,
                )
                .await
                .unwrap();
        }
        audited.commit().await.unwrap();

        let entries = archive.audit_log(None, None).await.unwrap();
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(
            entries[0].entity.as_ref().unwrap().label,
            format!("p{}", MAX_ENTRIES + 2)
        );
        assert_eq!(
            entries[MAX_ENTRIES - 1].entity.as_ref().unwrap().label,
            "p3"
        );
    }

    /// The cursor hands back the rest without a gap or an overlap
    #[tokio::test]
    async fn a_cursor_continues_below_the_last_entry_read() {
        let archive = Archive::in_memory().await.unwrap();
        let mut audited = archive.shared().begin_write().await.unwrap();
        for n in 0..5 {
            audited
                .record(
                    Source::User,
                    Action::Updated,
                    publication(&format!("p{n}")),
                    None,
                )
                .await
                .unwrap();
        }
        audited.commit().await.unwrap();

        let labels = |entries: Vec<AuditEntry>| -> Vec<String> {
            entries
                .into_iter()
                .map(|entry| entry.entity.unwrap().label)
                .collect()
        };
        let first = archive.audit_log(None, Some(2)).await.unwrap();
        let cursor = first.last().unwrap().id;
        assert_eq!(labels(first), ["p4", "p3"]);
        let second = archive.audit_log(Some(cursor), Some(2)).await.unwrap();
        let cursor = second.last().unwrap().id;
        assert_eq!(labels(second), ["p2", "p1"]);
        assert_eq!(
            labels(archive.audit_log(Some(cursor), Some(2)).await.unwrap()),
            ["p0"]
        );
    }
}
