use std::fmt;
use std::num::ParseIntError;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;

use sqlx::{Sqlite, SqliteConnection, Transaction};

use crate::entity::{EntityKind, Timestamp};
use crate::library::LibraryId;
use crate::{Engine, Error, Result};

const MAX_EVENTS: usize = 3_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AuditId(i64);

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
}

impl fmt::Display for AuditSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            AuditSource::User => "user",
            AuditSource::OrphanCleanup => "orphan_cleanup",
            AuditSource::Merge => "merge",
            AuditSource::LinkRecognition => "link_recognition",
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
        kind: EntityKind,
        id: i64,
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
    pub field: Option<String>,
}

pub(crate) struct Audited<'a> {
    tx: Transaction<'a, Sqlite>,
}

impl<'a> Audited<'a> {
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
        Ok(Recorded { tx: self.tx })
    }
}

pub(crate) struct Recorded<'a> {
    tx: Transaction<'a, Sqlite>,
}

impl<'a> Recorded<'a> {
    #[cfg_attr(not(test), expect(dead_code))]
    pub(crate) async fn record(
        mut self,
        source: AuditSource,
        action: Action,
        subject: Option<&AuditSubject>,
        field: Option<&str>,
    ) -> Result<Recorded<'a>> {
        insert(&mut self.tx, source, action, subject, field).await?;
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

async fn insert(
    tx: &mut SqliteConnection,
    source: AuditSource,
    action: Action,
    subject: Option<&AuditSubject>,
    field: Option<&str>,
) -> Result<()> {
    let (kind, id, library, label) = match subject {
        None => (None, None, None, None),
        Some(AuditSubject::Library { id, label }) => {
            (Some("library".to_string()), Some(id.0), None, Some(label))
        }
        Some(AuditSubject::Entity {
            kind,
            id,
            library,
            label,
        }) => (
            Some(kind.to_string()),
            Some(*id),
            Some(library.0),
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

impl Engine {
    /// Begin a write transaction that must record an audit event before it can be committed
    pub(crate) async fn begin_write(&self) -> Result<Audited<'static>> {
        Ok(Audited {
            tx: self.inner.pool.begin_with("BEGIN IMMEDIATE").await?,
        })
    }

    /// Read audit events newest first, below `before` when given, at most `limit`
    pub async fn audit_log(
        &self,
        before: Option<AuditId>,
        limit: usize,
    ) -> Result<Vec<AuditEvent>> {
        let before = before.map(|id| id.0);
        // SQLite reads a negative LIMIT as unlimited, which a wrapping cast of a huge limit yields.
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut conn = self.inner.pool.acquire().await?;
        let rows = sqlx::query!(
            r#"SELECT id, created_at AS "created_at: Timestamp", source, action,
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
                let missing = |column| eyre::eyre!("audit event {} has no {column}", row.id);
                let label = row.entity_label.unwrap_or_default();
                let subject = match row.entity_kind.as_deref() {
                    None => None,
                    Some("library") => Some(AuditSubject::Library {
                        id: LibraryId(row.entity_id.ok_or_else(|| missing("entity_id"))?),
                        label,
                    }),
                    Some(kind) => Some(AuditSubject::Entity {
                        kind: kind.parse()?,
                        id: row.entity_id.ok_or_else(|| missing("entity_id"))?,
                        library: LibraryId(
                            row.entity_library_id
                                .ok_or_else(|| missing("entity_library_id"))?,
                        ),
                        label,
                    }),
                };
                Ok(AuditEvent {
                    id: AuditId(row.id),
                    created_at: row.created_at,
                    source: row.source.parse()?,
                    action: row.action.parse()?,
                    subject,
                    field: row.field,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn audit_events_round_trip() {
        let engine = Engine::demo().await.unwrap();
        let library = AuditSubject::Library {
            id: LibraryId(1),
            label: "Scores".into(),
        };
        let work = AuditSubject::Entity {
            kind: EntityKind::Work,
            id: 7,
            library: LibraryId(1),
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

        let events = engine.audit_log(None, 10).await.unwrap();
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
    }
}
