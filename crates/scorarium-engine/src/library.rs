use std::fmt;
use std::num::ParseIntError;
use std::str::FromStr;

use sqlx::SqliteConnection;

use crate::audit::{Action, AuditSource, AuditSubject};
use crate::contributor::Role;
use crate::entity::AnyEntityId;
use crate::holding::NewHolding;
use crate::person::{self, Person, PersonId, PersonSummary};
use crate::publication::{self, ImportSeed, Publication, PublicationId};
use crate::suggest::{self, SuggestField, Suggestion};
use crate::value::Tag;
use crate::work::{self, Work};
use crate::{Engine, Error, Result, asset};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LibraryId(i64);

impl LibraryId {
    pub(crate) fn new(id: i64) -> LibraryId {
        LibraryId(id)
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

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
        let mut conn = self.engine.begin_read().await?;
        self.refresh(&mut conn).await
    }

    /// Overwrite the cached fields with the row as `conn` sees it
    ///
    /// Mutators call this inside their write transaction so that the unchanged-edit check and the
    /// audit label come from the locked row rather than from a stale handle.
    async fn refresh(&mut self, conn: &mut SqliteConnection) -> Result<()> {
        let row = sqlx::query!(
            r#"SELECT name, private AS "private: bool" FROM library WHERE id = ?"#,
            self.id.get()
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
        sqlx::query!(
            "UPDATE library SET name = ? WHERE id = ?",
            name,
            self.id.get()
        )
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
            self.id.get()
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
        let publications = sqlx::query_scalar!(
            "SELECT id FROM publication WHERE library_id = ?",
            self.id.get()
        )
        .fetch_all(&mut *tx)
        .await?;
        let people =
            sqlx::query_scalar!("SELECT id FROM person WHERE library_id = ?", self.id.get())
                .fetch_all(&mut *tx)
                .await?;
        let name = sqlx::query_scalar!(
            "DELETE FROM library WHERE id = ? RETURNING name",
            self.id.get()
        )
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
        .await?;
        for id in publications {
            asset::remove_dir(
                &self.engine,
                AnyEntityId::Publication(PublicationId::new(id)),
            );
        }
        for id in people {
            asset::remove_dir(&self.engine, AnyEntityId::Person(PersonId::new(id)));
        }
        Ok(())
    }
}

impl Library {
    /// Import a draft publication from `seed` with its first holding
    pub async fn import(&self, seed: ImportSeed, holding: NewHolding<'_>) -> Result<Publication> {
        publication::import(&self.engine, self.id, seed, holding).await
    }

    /// List every publication in the library, drafts included, oldest first
    pub async fn publications(&self) -> Result<Vec<Publication>> {
        publication::in_library(&self.engine, self.id).await
    }

    /// List every work in the library
    pub async fn works(&self) -> Result<Vec<Work>> {
        work::in_library(&self.engine, self.id).await
    }

    /// List people in the library with the given role
    pub async fn people(&self, role: Option<&Role>) -> Result<Vec<Person>> {
        person::in_library(&self.engine, self.id, role).await
    }

    /// Summarize the people in the library, alphabetically by sort name
    pub async fn people_summary(
        &self,
        role: Option<&Role>,
        include_drafts: bool,
    ) -> Result<Vec<PersonSummary>> {
        person::summarize(&self.engine, self.id, role, include_drafts).await
    }

    /// Count the publications and works with each tag in the library, alphabetically by tag
    ///
    /// Leave drafts out of the counts unless `include_drafts` is set.
    pub async fn tags(&self, include_drafts: bool) -> Result<Vec<(Tag, usize)>> {
        let mut conn = self.engine.begin_read().await?;
        tag_counts(&mut conn, self.id, include_drafts).await
    }

    /// Count the publications in the library
    ///
    /// Leave drafts out unless `include_drafts` is set.
    pub async fn publication_count(&self, include_drafts: bool) -> Result<usize> {
        let mut conn = self.engine.begin_read().await?;
        let row = sqlx::query!(
            r#"SELECT COUNT(*) AS "count: u32" FROM publication
               WHERE library_id = ? AND (? OR draft = 0)"#,
            self.id.get(),
            include_drafts
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(row.count as usize)
    }

    /// Count the people holding each role in the library, alphabetically by role
    pub async fn roles(&self, include_drafts: bool) -> Result<Vec<(Role, usize)>> {
        let mut conn = self.engine.begin_read().await?;
        role_counts(&mut conn, self.id, include_drafts).await
    }

    /// List the publications and works in the library tagged `tag`, oldest first
    pub async fn tagged_with(&self, tag: &Tag) -> Result<(Vec<Publication>, Vec<Work>)> {
        let publications = publication::tagged(&self.engine, self.id, tag).await?;
        let works = work::tagged(&self.engine, self.id, tag).await?;
        Ok((publications, works))
    }

    /// Offer suggested values for a field from what the library already holds, best match first
    pub async fn suggest(
        &self,
        field: SuggestField,
        query: &str,
        limit: usize,
    ) -> Result<Vec<Suggestion>> {
        suggest::suggest(&self.engine, self.id, field, query, limit).await
    }
}

/// Count the publications and works with each tag in `library`, alphabetically by tag
pub(crate) async fn tag_counts(
    conn: &mut SqliteConnection,
    library: LibraryId,
    include_drafts: bool,
) -> Result<Vec<(Tag, usize)>> {
    let rows = sqlx::query!(
        r#"SELECT tag.tag, COUNT(*) AS "count: u32"
           FROM tag
           LEFT JOIN publication ON publication.id = tag.publication_id
           LEFT JOIN work ON work.id = tag.work_id
           WHERE tag.library_id = ?
             AND (? OR COALESCE(publication.draft, work.draft) = 0)
           GROUP BY tag.tag
           ORDER BY tag.tag"#,
        library.get(),
        include_drafts
    )
    .fetch_all(conn)
    .await?;
    rows.into_iter()
        .map(|row| Ok((row.tag.parse()?, row.count as usize)))
        .collect()
}

pub(crate) async fn role_counts(
    conn: &mut SqliteConnection,
    library: LibraryId,
    include_drafts: bool,
) -> Result<Vec<(Role, usize)>> {
    let rows = sqlx::query!(
        r#"SELECT held.role, COUNT(DISTINCT held.person_id) AS "count: u32"
           FROM (
               SELECT pc.person_id, pc.role, publication.draft
               FROM publication_contributor AS pc
               JOIN publication ON publication.id = pc.publication_id
               WHERE pc.library_id = ?1
               UNION ALL
               SELECT wc.person_id, wc.role, work.draft
               FROM work_contributor AS wc
               JOIN work ON work.id = wc.work_id
               WHERE wc.library_id = ?1
           ) AS held
           JOIN person ON person.id = held.person_id
           WHERE ?2 OR (person.draft = 0 AND held.draft = 0)
           GROUP BY held.role
           ORDER BY held.role"#,
        library.get(),
        include_drafts
    )
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (Role::from_valid(row.role), row.count as usize))
        .collect())
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
        let mut conn = self.begin_read().await?;
        let rows =
            sqlx::query!(r#"SELECT id, name, private AS "private: bool" FROM library ORDER BY id"#)
                .fetch_all(&mut *conn)
                .await?;
        Ok(rows
            .into_iter()
            .map(|row| Library {
                engine: self.clone(),
                id: LibraryId::new(row.id),
                name: row.name,
                private: row.private,
            })
            .collect())
    }

    /// Load the library with the given id, if it exists
    pub async fn library(&self, id: LibraryId) -> Result<Option<Library>> {
        let mut conn = self.begin_read().await?;
        let row = sqlx::query!(
            r#"SELECT name, private AS "private: bool" FROM library WHERE id = ?"#,
            id.get()
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
            id: LibraryId::new(id),
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
    use crate::entity::Entity;
    use crate::holding::NewHolding;
    use crate::publication::ImportSeed;
    use crate::{Engine, Error};

    #[tokio::test]
    async fn create_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
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
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        let mut library = engine.create_library("Scores", false).await.unwrap();
        library.rename("Scores").await.unwrap();
        library.rename("Music").await.unwrap();
        library.set_private(true).await.unwrap();
        library.delete().await.unwrap();

        let events: Vec<_> = engine
            .audit_log(0, 10)
            .await
            .unwrap()
            .events
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

    #[tokio::test]
    async fn roles_count_people_once_per_role() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), crate::test::options())
            .await
            .unwrap();
        let library = engine.create_library("Sheet music", false).await.unwrap();
        let seed = |title: &str| ImportSeed::Title {
            title: title.into(),
            record: None,
        };

        // Satie is composer on both works, and editor on the publication and on one work, so the
        // count has to be distinct across both contributor tables.
        let mut gymnopedies = library
            .import(
                seed("Three gymnopedies"),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        let satie = gymnopedies
            .create_contributor("Erik Satie", "editor".parse().unwrap())
            .await
            .unwrap();
        let mut first = gymnopedies.create_work("Gymnopedie No. 1").await.unwrap();
        let mut second = gymnopedies.create_work("Gymnopedie No. 2").await.unwrap();
        first
            .add_contributor(satie.id(), "composer".parse().unwrap())
            .await
            .unwrap();
        first
            .add_contributor(satie.id(), "editor".parse().unwrap())
            .await
            .unwrap();
        second
            .add_contributor(satie.id(), "composer".parse().unwrap())
            .await
            .unwrap();
        gymnopedies.commit().await.unwrap();

        // An imported publication is a draft, and so are the people created through it.
        let mut draft = library
            .import(seed("Untitled"), NewHolding::Physical { location: None })
            .await
            .unwrap();
        let pen_name = draft
            .create_contributor("Pen Name", "author".parse().unwrap())
            .await
            .unwrap();
        assert!(pen_name.is_draft());
        draft
            .add_contributor(satie.id(), "arranger".parse().unwrap())
            .await
            .unwrap();

        assert_eq!(
            library.roles(false).await.unwrap(),
            vec![
                ("composer".parse().unwrap(), 1),
                ("editor".parse().unwrap(), 1)
            ]
        );
        assert_eq!(
            library.roles(true).await.unwrap(),
            vec![
                ("arranger".parse().unwrap(), 1),
                ("author".parse().unwrap(), 1),
                ("composer".parse().unwrap(), 1),
                ("editor".parse().unwrap(), 1)
            ]
        );
    }
}
