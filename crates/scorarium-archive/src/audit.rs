use std::ops::{Deref, DerefMut};

use sqlx::{Sqlite, SqliteConnection, Transaction};

/// How many groups of events the log keeps
pub(crate) const MAX_GROUPS: usize = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// An event the user initiated
    User,
    /// An event triggered by doing orphan cleanup
    OrphanCleanup,
    /// An event triggered by merging two entities
    Merge,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::User => "user",
            Source::OrphanCleanup => "orphan_cleanup",
            Source::Merge => "merge",
        }
    }

    fn parse(text: &str) -> crate::Result<Source> {
        Ok(match text {
            "user" => Source::User,
            "orphan_cleanup" => Source::OrphanCleanup,
            "merge" => Source::Merge,
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
    Renamed,
    ImportStarted,
    ImportAccepted,
    ImportDiscarded,
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
            Action::Renamed => "renamed",
            Action::ImportStarted => "import_started",
            Action::ImportAccepted => "import_accepted",
            Action::ImportDiscarded => "import_discarded",
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
            "renamed" => Action::Renamed,
            "import_started" => Action::ImportStarted,
            "import_accepted" => Action::ImportAccepted,
            "import_discarded" => Action::ImportDiscarded,
            "password_claimed" => Action::PasswordClaimed,
            "password_changed" => Action::PasswordChanged,
            other => eyre::bail!("unknown audit action {other:?}"),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntityKind {
    Publication,
    Work,
    Person,
    Library,
    Import,
}

impl EntityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EntityKind::Publication => "publication",
            EntityKind::Work => "work",
            EntityKind::Person => "person",
            EntityKind::Library => "library",
            EntityKind::Import => "import",
        }
    }

    fn parse(text: &str) -> crate::Result<EntityKind> {
        Ok(match text {
            "publication" => EntityKind::Publication,
            "work" => EntityKind::Work,
            "person" => EntityKind::Person,
            "library" => EntityKind::Library,
            "import" => EntityKind::Import,
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
            other => eyre::bail!("unknown audit field {other:?}"),
        })
    }
}

/// What an entry points at, when it points at anything
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityRef {
    pub kind: EntityKind,
    pub id: i64,
    pub library_id: Option<i64>,
    /// The entity's name as it read when the entry was written, so a deleted entity still renders
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub action: Action,
    /// None when the event names nothing browsable, i.e. a password change
    pub entity: Option<EntityRef>,
    /// Only ever non-empty for Action::Updated
    pub fields: Vec<Field>,
}

impl Event {
    /// An event that names no entity, or one whose entity is not known until the mutation has run
    pub fn new(action: Action) -> Event {
        Event {
            action,
            entity: None,
            fields: Vec::new(),
        }
    }

    pub fn about(action: Action, entity: EntityRef) -> Event {
        Event {
            action,
            entity: Some(entity),
            fields: Vec::new(),
        }
    }
}

/// One row of the audit log
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuditEntry {
    pub id: i64,
    /// None on the entry that heads a group
    pub group_id: Option<i64>,
    pub created_at: i64,
    pub source: Source,
    pub event: Event,
}

/// A write transaction whose headline entry is already recorded.
pub(crate) struct Audited<'a> {
    tx: Transaction<'a, Sqlite>,
    /// The group ID that this handle's consequences hang from
    group: i64,
}

impl<'a> Audited<'a> {
    pub(crate) fn new(tx: Transaction<'a, Sqlite>, group: i64) -> Audited<'a> {
        Audited { tx, group }
    }

    /// Record a consequence of the action this handle was opened for
    pub(crate) async fn record(&mut self, source: Source, event: Event) -> crate::Result<()> {
        insert(&mut self.tx, Some(self.group), source, &event).await?;
        Ok(())
    }

    /// Fill in the headline's entity once the mutation has produced it.
    ///
    /// The headline is written before the mutation runs, so an action that creates its entity
    /// cannot name the id up front.
    pub(crate) async fn set_entity(&mut self, entity: &EntityRef) -> crate::Result<()> {
        let kind = entity.kind.as_str();
        sqlx::query!(
            "UPDATE audit_entry
             SET entity_kind = ?, entity_id = ?, entity_library_id = ?, entity_label = ?
             WHERE id = ?",
            kind,
            entity.id,
            entity.library_id,
            entity.label,
            self.group
        )
        .execute(&mut *self.tx)
        .await?;
        Ok(())
    }

    /// Fill in the headline's field list once the mutation has run and the diff is known
    pub(crate) async fn set_fields(&mut self, fields: &[Field]) -> crate::Result<()> {
        let names: Vec<&str> = fields.iter().map(|field| field.as_str()).collect();
        let json = (!names.is_empty())
            .then(|| serde_json::to_string(&names))
            .transpose()?;
        sqlx::query!(
            "UPDATE audit_entry SET fields = ? WHERE id = ?",
            json,
            self.group
        )
        .execute(&mut *self.tx)
        .await?;
        Ok(())
    }

    pub(crate) async fn commit(self) -> crate::Result<()> {
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

/// Write one entry, returning its id. `group` is None for a headline.
pub(crate) async fn insert(
    conn: &mut SqliteConnection,
    group: Option<i64>,
    source: Source,
    event: &Event,
) -> crate::Result<i64> {
    let source = source.as_str();
    let action = event.action.as_str();
    let kind = event.entity.as_ref().map(|e| e.kind.as_str());
    let id = event.entity.as_ref().map(|e| e.id);
    let library_id = event.entity.as_ref().and_then(|e| e.library_id);
    let label = event.entity.as_ref().map(|e| e.label.as_str());
    let fields = if event.fields.is_empty() {
        None
    } else {
        let names: Vec<&str> = event.fields.iter().map(|f| f.as_str()).collect();
        Some(serde_json::to_string(&names)?)
    };
    let written = sqlx::query!(
        "INSERT INTO audit_entry
             (group_id, source, action, entity_kind, entity_id, entity_library_id, entity_label, fields)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        group,
        source,
        action,
        kind,
        id,
        library_id,
        label,
        fields
    )
    .execute(&mut *conn)
    .await?;
    Ok(written.last_insert_rowid())
}

/// Keep the newest [MAX_GROUPS] groups
pub(crate) async fn trim(conn: &mut SqliteConnection) -> crate::Result<()> {
    let keep = MAX_GROUPS as i64;
    sqlx::query!(
        "DELETE FROM audit_entry
         WHERE group_id IS NULL
           AND id NOT IN (SELECT id FROM audit_entry WHERE group_id IS NULL ORDER BY id DESC LIMIT ?)",
        keep
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Every entry, newest group first, each group's headline ahead of its consequences
pub(crate) async fn load_entries(conn: &mut SqliteConnection) -> crate::Result<Vec<AuditEntry>> {
    let rows = sqlx::query!(
        r#"SELECT id, group_id, created_at, source, action,
                  entity_kind, entity_id, entity_library_id, entity_label, fields
           FROM audit_entry
           ORDER BY COALESCE(group_id, id) DESC, id ASC"#
    )
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter()
        .map(|row| {
            let entity = match row.entity_kind {
                None => None,
                Some(kind) => Some(EntityRef {
                    kind: EntityKind::parse(&kind)?,
                    id: row
                        .entity_id
                        .ok_or_else(|| eyre::eyre!("audit entry {} names no entity id", row.id))?,
                    library_id: row.entity_library_id,
                    label: row.entity_label.unwrap_or_default(),
                }),
            };
            let fields = match row.fields {
                None => Vec::new(),
                Some(json) => serde_json::from_str::<Vec<String>>(&json)?
                    .iter()
                    .map(|name| Field::parse(name))
                    .collect::<crate::Result<_>>()?,
            };
            Ok(AuditEntry {
                id: row.id,
                group_id: row.group_id,
                created_at: row.created_at,
                source: Source::parse(&row.source)?,
                event: Event {
                    action: Action::parse(&row.action)?,
                    entity,
                    fields,
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Archive;

    fn user_event(action: Action, label: &str) -> Event {
        Event {
            action,
            entity: Some(EntityRef {
                kind: EntityKind::Publication,
                id: 1,
                library_id: Some(1),
                label: label.to_string(),
            }),
            fields: Vec::new(),
        }
    }

    #[tokio::test]
    async fn a_handle_records_a_headline_and_its_consequences() {
        let archive = Archive::in_memory().await.unwrap();
        let mut audited = archive
            .shared()
            .begin_audit(Source::User, user_event(Action::Updated, "Nocturnes"))
            .await
            .unwrap();
        audited
            .record(Source::OrphanCleanup, user_event(Action::Deleted, "Bach"))
            .await
            .unwrap();
        audited.commit().await.unwrap();

        let entries = archive.audit_log().await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].group_id, None);
        assert_eq!(entries[1].group_id, Some(entries[0].id));
        assert_eq!(entries[1].source, Source::OrphanCleanup);
    }

    #[tokio::test]
    async fn trimming_drops_whole_groups() {
        let archive = Archive::in_memory().await.unwrap();
        for n in 0..(MAX_GROUPS + 3) {
            let mut audited = archive
                .shared()
                .begin_audit(Source::User, user_event(Action::Updated, &format!("p{n}")))
                .await
                .unwrap();
            audited
                .record(Source::Merge, user_event(Action::Merged, "dupe"))
                .await
                .unwrap();
            audited.commit().await.unwrap();
        }
        let entries = archive.audit_log().await.unwrap();
        // Two entries per group, and no consequence left without its headline
        assert_eq!(entries.len(), MAX_GROUPS * 2);
        assert!(entries.iter().all(|entry| match entry.group_id {
            None => true,
            Some(group) => entries.iter().any(|other| other.id == group),
        }));
    }
}
