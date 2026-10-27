use std::fmt;
use std::num::ParseIntError;
use std::str::FromStr;

use sqlx::SqliteConnection;

use crate::audit::{Action, AuditSource, AuditSubject};
use crate::{Engine, Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LibraryId(pub(crate) i64);

impl fmt::Display for LibraryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for LibraryId {
    type Err = ParseIntError;

    fn from_str(text: &str) -> std::result::Result<LibraryId, ParseIntError> {
        text.parse().map(LibraryId)
    }
}

#[derive(Clone, Debug)]
pub struct Library {
    engine: Engine,
    id: LibraryId,
    name: String,
    private: bool,
}

impl Library {
    pub fn id(&self) -> LibraryId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_private(&self) -> bool {
        self.private
    }

    /// Re-read the library from the database
    ///
    /// Returns [Error::NotFound] if the library has been deleted.
    pub async fn reload(&mut self) -> Result<()> {
        let mut conn = self.engine.inner.pool.acquire().await?;
        self.refresh(&mut conn).await
    }

    /// Overwrite the cached fields with the row as `conn` sees it
    ///
    /// Mutators call this inside their write transaction so that the unchanged-edit check and the
    /// audit label come from the locked row rather than from a stale handle.
    async fn refresh(&mut self, conn: &mut SqliteConnection) -> Result<()> {
        let row = sqlx::query!(
            r#"SELECT name, private AS "private: bool" FROM library WHERE id = ?"#,
            self.id.0
        )
        .fetch_optional(conn)
        .await?
        .ok_or(Error::NotFound)?;
        self.name = row.name;
        self.private = row.private;
        Ok(())
    }

    /// Rename the library
    ///
    /// Returns [Error::NotFound] if the library has been deleted.
    pub async fn rename(&mut self, name: impl AsRef<str>) -> Result<()> {
        let name = clean_name(name.as_ref())?;
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.name == name {
            return Ok(());
        }
        sqlx::query!("UPDATE library SET name = ? WHERE id = ?", name, self.id.0)
            .execute(&mut *tx)
            .await?;
        tx.record(
            AuditSource::User,
            Action::Updated,
            Some(&audit_subject(self.id, &self.name)),
            Some("name"),
        )
        .await?
        .commit()
        .await?;
        self.name = name.to_string();
        Ok(())
    }

    /// Hide the library from unauthenticated users, or show it
    ///
    /// Returns [Error::NotFound] if the library has been deleted.
    pub async fn set_private(&mut self, private: bool) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.private == private {
            return Ok(());
        }
        sqlx::query!(
            "UPDATE library SET private = ? WHERE id = ?",
            private,
            self.id.0
        )
        .execute(&mut *tx)
        .await?;
        tx.record(
            AuditSource::User,
            Action::Updated,
            Some(&audit_subject(self.id, &self.name)),
            Some("visibility"),
        )
        .await?
        .commit()
        .await?;
        self.private = private;
        Ok(())
    }

    /// Delete the library and everything in it
    ///
    /// Returns [Error::NotFound] if the library has already been deleted.
    pub async fn delete(self) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        let name =
            sqlx::query_scalar!("DELETE FROM library WHERE id = ? RETURNING name", self.id.0)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(Error::NotFound)?;
        tx.record(
            AuditSource::User,
            Action::Deleted,
            Some(&audit_subject(self.id, &name)),
            None,
        )
        .await?
        .commit()
        .await
    }
}

fn audit_subject(id: LibraryId, label: &str) -> AuditSubject {
    AuditSubject::Library {
        id,
        label: label.to_string(),
    }
}

fn clean_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::InvalidValue(
            "A library name cannot be blank.".into(),
        ));
    }
    Ok(name)
}

impl Engine {
    /// List every library, oldest first
    pub async fn libraries(&self) -> Result<Vec<Library>> {
        let mut conn = self.inner.pool.acquire().await?;
        let rows =
            sqlx::query!(r#"SELECT id, name, private AS "private: bool" FROM library ORDER BY id"#)
                .fetch_all(&mut *conn)
                .await?;
        Ok(rows
            .into_iter()
            .map(|row| Library {
                engine: self.clone(),
                id: LibraryId(row.id),
                name: row.name,
                private: row.private,
            })
            .collect())
    }

    /// Load the library with the given id, if it exists
    pub async fn library(&self, id: LibraryId) -> Result<Option<Library>> {
        let mut conn = self.inner.pool.acquire().await?;
        let row = sqlx::query!(
            r#"SELECT name, private AS "private: bool" FROM library WHERE id = ?"#,
            id.0
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row.map(|row| Library {
            engine: self.clone(),
            id,
            name: row.name,
            private: row.private,
        }))
    }

    /// Create a library with the given name and visibility
    ///
    /// Names need not be unique.
    pub async fn create_library(&self, name: impl AsRef<str>, private: bool) -> Result<Library> {
        let name = clean_name(name.as_ref())?;
        let mut tx = self.begin_write().await?;
        let id = sqlx::query_scalar!(
            "INSERT INTO library (name, private) VALUES (?, ?) RETURNING id",
            name,
            private
        )
        .fetch_one(&mut *tx)
        .await?;
        let library = Library {
            engine: self.clone(),
            id: LibraryId(id),
            name: name.to_string(),
            private,
        };
        tx.record(
            AuditSource::User,
            Action::Created,
            Some(&audit_subject(library.id, &library.name)),
            None,
        )
        .await?
        .commit()
        .await?;
        Ok(library)
    }
}

#[cfg(test)]
mod tests {
    use crate::audit::{Action, AuditSubject};
    use crate::{Engine, Error};

    #[tokio::test]
    async fn create_and_load() {
        let engine = Engine::demo().await.unwrap();
        let created = engine.create_library("  Scores ", true).await.unwrap();
        let loaded = engine.library(created.id()).await.unwrap().unwrap();
        assert_eq!(loaded.name(), "Scores");
        assert!(loaded.is_private());
        assert!(matches!(
            engine.create_library("   ", false).await,
            Err(Error::InvalidValue(_))
        ));
    }

    #[tokio::test]
    async fn edits_audit_once_per_change() {
        let engine = Engine::demo().await.unwrap();
        let mut library = engine.create_library("Scores", false).await.unwrap();
        library.rename("Scores").await.unwrap();
        library.rename("Music").await.unwrap();
        library.set_private(true).await.unwrap();
        library.delete().await.unwrap();

        let events: Vec<_> = engine
            .audit_log(None, 10)
            .await
            .unwrap()
            .into_iter()
            .map(|e| {
                let Some(AuditSubject::Library { label, .. }) = e.subject else {
                    panic!("not a library event");
                };
                (e.action, label, e.field)
            })
            .collect();
        assert_eq!(
            events,
            [
                (Action::Deleted, "Music".into(), None),
                (Action::Updated, "Music".into(), Some("visibility".into())),
                (Action::Updated, "Scores".into(), Some("name".into())),
                (Action::Created, "Scores".into(), None),
            ]
        );
    }
}
