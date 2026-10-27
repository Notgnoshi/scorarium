use std::fmt;
use std::num::ParseIntError;
use std::str::FromStr;

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

    fn audit_subject(&self) -> AuditSubject {
        AuditSubject::Library {
            id: self.id,
            label: self.name.clone(),
        }
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
            id: LibraryId(id),
            name: name.to_string(),
            private,
        };
        tx.record(
            AuditSource::User,
            Action::Created,
            Some(&library.audit_subject()),
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
}
