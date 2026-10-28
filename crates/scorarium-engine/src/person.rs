use std::fmt;
use std::num::ParseIntError;
use std::str::FromStr;

use sqlx::SqliteConnection;

use crate::audit::{Action, AuditEvent, AuditId, AuditSource, AuditSubject, Audited};
use crate::entity::{Entity, EntityKind, Timestamp};
use crate::library::LibraryId;
use crate::link::{ExternalRecord, HttpUrl, Link, LinkId};
use crate::publication::{self, Publication, PublicationId};
use crate::work::{self, Work, WorkId};
use crate::{Engine, Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PersonId(i64);

impl PersonId {
    pub(crate) fn new(id: i64) -> PersonId {
        PersonId(id)
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

impl fmt::Display for PersonId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for PersonId {
    type Err = ParseIntError;

    fn from_str(text: &str) -> std::result::Result<PersonId, ParseIntError> {
        text.parse().map(PersonId)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    Name,
    Links,
    Profile,
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Field::Name => "name",
            Field::Links => "links",
            Field::Profile => "profile",
        })
    }
}

impl FromStr for Field {
    type Err = Error;

    fn from_str(text: &str) -> Result<Field> {
        Ok(match text {
            "name" => Field::Name,
            "links" => Field::Links,
            "profile" => Field::Profile,
            other => return Err(eyre::eyre!("unknown person field {other:?}").into()),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Person {
    engine: Engine,
    id: PersonId,
    library: LibraryId,
    draft: bool,
    created_at: Timestamp,
    last_modified_at: Timestamp,
    name: String,
    sort_name: String,
    links: Vec<Link>,
}

/// A POD type to serialize a person from the database into
struct PersonRow {
    id: i64,
    library_id: i64,
    name: String,
    sort_name: String,
    draft: bool,
    created_at: Timestamp,
    last_modified_at: Timestamp,
}

impl Person {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn sort_name(&self) -> &str {
        &self.sort_name
    }

    /// List the person's links in the order they were added
    pub fn links(&self) -> &[Link] {
        &self.links
    }

    async fn from_row(
        engine: &Engine,
        conn: &mut SqliteConnection,
        row: PersonRow,
    ) -> Result<Person> {
        let links = sqlx::query!(
            "SELECT id, url, kind, external_id FROM person_link WHERE person_id = ? ORDER BY id",
            row.id
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|link| Link::new(link.id, &link.url, &link.kind, link.external_id))
        .collect::<Result<_>>()?;
        Ok(Person {
            engine: engine.clone(),
            id: PersonId::new(row.id),
            library: LibraryId::new(row.library_id),
            draft: row.draft,
            created_at: row.created_at,
            last_modified_at: row.last_modified_at,
            name: row.name,
            sort_name: row.sort_name,
            links,
        })
    }

    async fn fetch(
        engine: &Engine,
        conn: &mut SqliteConnection,
        id: PersonId,
    ) -> Result<Option<Person>> {
        let row = sqlx::query_as!(
            PersonRow,
            r#"SELECT id, library_id, name, sort_name, draft AS "draft: bool",
                      created_at AS "created_at: Timestamp",
                      last_modified_at AS "last_modified_at: Timestamp"
               FROM person WHERE id = ?"#,
            id.get()
        )
        .fetch_optional(&mut *conn)
        .await?;
        match row {
            Some(row) => Ok(Some(Person::from_row(engine, conn, row).await?)),
            None => Ok(None),
        }
    }

    async fn refresh(&mut self, conn: &mut SqliteConnection) -> Result<()> {
        *self = Person::fetch(&self.engine, conn, self.id)
            .await?
            .ok_or(Error::NotFound)?;
        Ok(())
    }

    pub(crate) fn audit_subject(&self) -> AuditSubject {
        audit_subject(self.id, self.library, &self.name)
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

    /// Rename the person along with the given sort name
    ///
    /// Returns [Error::NotFound] if the person has been deleted.
    pub async fn rename(
        &mut self,
        name: impl AsRef<str>,
        sort_name: impl AsRef<str>,
    ) -> Result<()> {
        let name = clean_name(name.as_ref())?;
        let sort_name = sort_name.as_ref().trim();
        if sort_name.is_empty() {
            return Err(Error::InvalidValue("A sort name cannot be blank.".into()));
        }
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self.name == name && self.sort_name == sort_name {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE person SET name = ?, sort_name = ?, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            name,
            sort_name,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Name).await?;
        self.name = name.to_string();
        self.sort_name = sort_name.to_string();
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Link the person to `url`
    ///
    /// Returns [Error::NotFound] if the person has been deleted.
    pub async fn add_link(&mut self, url: HttpUrl) -> Result<()> {
        let record = ExternalRecord::recognize(EntityKind::Person, &url);
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if self
            .links
            .iter()
            .any(|link| link.url == url || (link.record.is_some() && link.record == record))
        {
            return Ok(());
        }
        let kind = record
            .as_ref()
            .map_or_else(|| "generic".to_string(), |record| record.kind().to_string());
        let external_id = record.as_ref().map(ExternalRecord::id);
        let id = sqlx::query_scalar!(
            r#"INSERT INTO person_link (person_id, url, kind, external_id) VALUES (?, ?, ?, ?)
               RETURNING id AS "id!""#,
            self.id.get(),
            url.as_url().as_str(),
            kind,
            external_id
        )
        .fetch_one(&mut *tx)
        .await?;
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE person SET last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Links).await?;
        self.links.push(Link {
            id: LinkId::new(id),
            url,
            record,
        });
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// Remove a link from the person
    ///
    /// Returns [Error::NotFound] if the person has been deleted.
    pub async fn remove_link(&mut self, link: LinkId) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        let removed = sqlx::query!(
            "DELETE FROM person_link WHERE id = ? AND person_id = ?",
            link.get(),
            self.id.get()
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if removed == 0 {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE person SET last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        self.commit_update(tx, Field::Links).await?;
        self.links.retain(|existing| existing.id != link);
        self.last_modified_at = last_modified_at;
        Ok(())
    }

    /// List every publication that names the person as a contributor
    pub async fn publications(&self) -> Result<Vec<Publication>> {
        publication::with_contributor(&self.engine, self.id).await
    }

    /// List every work that names the person as a contributor
    pub async fn works(&self) -> Result<Vec<Work>> {
        work::with_contributor(&self.engine, self.id).await
    }
}

impl Entity for Person {
    type Id = PersonId;
    type Field = Field;

    async fn load(engine: &Engine, id: PersonId) -> Result<Option<Person>> {
        let mut conn = engine.begin_read().await?;
        Person::fetch(engine, &mut conn, id).await
    }

    async fn reload(&mut self) -> Result<()> {
        let mut conn = self.engine.begin_read().await?;
        self.refresh(&mut conn).await
    }

    fn id(&self) -> PersonId {
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
            .entity_audit_log(
                EntityKind::Person,
                self.id.get(),
                self.library,
                before,
                limit,
            )
            .await
    }

    async fn commit(&mut self) -> Result<()> {
        let mut tx = self.engine.begin_write().await?;
        self.refresh(&mut tx).await?;
        if !self.draft {
            return Ok(());
        }
        let last_modified_at = sqlx::query_scalar!(
            r#"UPDATE person SET draft = 0, last_modified_at = unixepoch() WHERE id = ?
               RETURNING last_modified_at AS "last_modified_at: Timestamp""#,
            self.id.get()
        )
        .fetch_one(&mut *tx)
        .await?;
        tx.record(
            AuditSource::User,
            Action::Committed,
            Some(&self.audit_subject()),
            None,
        )
        .await?
        .commit()
        .await?;
        self.draft = false;
        self.last_modified_at = last_modified_at;
        Ok(())
    }
}

/// Insert a person named `name` in `library` with a derived sort name and load them
pub(crate) async fn insert(
    engine: &Engine,
    conn: &mut SqliteConnection,
    library: LibraryId,
    name: &str,
    draft: bool,
) -> Result<Person> {
    let library = library.get();
    let sort_name = sort_name(name);
    let id = sqlx::query_scalar!(
        r#"INSERT INTO person (library_id, name, sort_name, draft) VALUES (?, ?, ?, ?)
           RETURNING id AS "id!""#,
        library,
        name,
        sort_name,
        draft
    )
    .fetch_one(&mut *conn)
    .await?;
    Person::fetch(engine, conn, PersonId::new(id))
        .await?
        .ok_or(Error::NotFound)
}

/// Get the name of the person with the id `person`, or none if `library` has no such person
pub(crate) async fn name(
    conn: &mut SqliteConnection,
    library: LibraryId,
    person: PersonId,
) -> Result<Option<String>> {
    let name = sqlx::query_scalar!(
        "SELECT name FROM person WHERE id = ? AND library_id = ?",
        person.get(),
        library.get()
    )
    .fetch_optional(conn)
    .await?;
    Ok(name)
}

/// List the people with the given `role` in `library`
pub(crate) async fn in_library(
    engine: &Engine,
    library: LibraryId,
    role: Option<&str>,
) -> Result<Vec<Person>> {
    let mut conn = engine.begin_read().await?;
    let library = library.get();
    let rows = match role.map(str::trim) {
        Some(role) => {
            sqlx::query_as!(
                PersonRow,
                r#"SELECT id, library_id, name, sort_name, draft AS "draft: bool",
                          created_at AS "created_at: Timestamp",
                          last_modified_at AS "last_modified_at: Timestamp"
                   FROM person
                   WHERE library_id = ?
                     AND (EXISTS (SELECT 1 FROM publication_contributor
                                  WHERE person_id = person.id AND role = ?)
                          OR EXISTS (SELECT 1 FROM work_contributor
                                     WHERE person_id = person.id AND role = ?))
                   ORDER BY id"#,
                library,
                role,
                role
            )
            .fetch_all(&mut *conn)
            .await?
        }
        None => {
            sqlx::query_as!(
                PersonRow,
                r#"SELECT id, library_id, name, sort_name, draft AS "draft: bool",
                          created_at AS "created_at: Timestamp",
                          last_modified_at AS "last_modified_at: Timestamp"
                   FROM person WHERE library_id = ? ORDER BY id"#,
                library
            )
            .fetch_all(&mut *conn)
            .await?
        }
    };
    let mut people = Vec::with_capacity(rows.len());
    for row in rows {
        people.push(Person::from_row(engine, &mut conn, row).await?);
    }
    Ok(people)
}

/// Clear the draft mark of every draft person named as a contributor of `publication`
///
/// Return the audit subject of each person committed, in id order.
pub(crate) async fn commit_contributors_of_publication(
    conn: &mut SqliteConnection,
    publication: PublicationId,
) -> Result<Vec<AuditSubject>> {
    let mut people = sqlx::query!(
        "UPDATE person SET draft = 0, last_modified_at = unixepoch()
         WHERE draft = 1
           AND id IN (SELECT person_id FROM publication_contributor WHERE publication_id = ?)
         RETURNING id, library_id, name",
        publication.get()
    )
    .fetch_all(conn)
    .await?;
    // SQLite does not define the order of RETURNING rows
    people.sort_by_key(|person| person.id);
    Ok(people
        .into_iter()
        .map(|person| {
            audit_subject(
                PersonId::new(person.id),
                LibraryId::new(person.library_id),
                &person.name,
            )
        })
        .collect())
}

/// Clear the draft mark of every draft person named as a contributor of `work`
///
/// Return the audit subject of each person committed, in id order.
pub(crate) async fn commit_contributors_of_work(
    conn: &mut SqliteConnection,
    work: WorkId,
) -> Result<Vec<AuditSubject>> {
    let mut people = sqlx::query!(
        "UPDATE person SET draft = 0, last_modified_at = unixepoch()
         WHERE draft = 1
           AND id IN (SELECT person_id FROM work_contributor WHERE work_id = ?)
         RETURNING id, library_id, name",
        work.get()
    )
    .fetch_all(conn)
    .await?;
    // SQLite does not define the order of RETURNING rows
    people.sort_by_key(|person| person.id);
    Ok(people
        .into_iter()
        .map(|person| {
            audit_subject(
                PersonId::new(person.id),
                LibraryId::new(person.library_id),
                &person.name,
            )
        })
        .collect())
}

/// Delete every person in `library` that no publication or work names as a contributor
///
/// Return the audit subject of each person deleted, in id order.
pub(crate) async fn collect_orphan_people(
    conn: &mut SqliteConnection,
    library: LibraryId,
) -> Result<Vec<AuditSubject>> {
    let mut people = sqlx::query!(
        "DELETE FROM person
         WHERE library_id = ?
           AND id NOT IN (SELECT person_id FROM publication_contributor)
           AND id NOT IN (SELECT person_id FROM work_contributor)
         RETURNING id AS \"id!\", name",
        library.get()
    )
    .fetch_all(conn)
    .await?;
    // SQLite does not define the order of RETURNING rows
    people.sort_by_key(|person| person.id);
    Ok(people
        .into_iter()
        .map(|person| audit_subject(PersonId::new(person.id), library, &person.name))
        .collect())
}

fn audit_subject(id: PersonId, library: LibraryId, name: &str) -> AuditSubject {
    AuditSubject::Entity {
        kind: EntityKind::Person,
        id: id.get(),
        library,
        label: name.to_string(),
    }
}

pub(crate) fn clean_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::InvalidValue(
            "A person's name cannot be blank.".into(),
        ));
    }
    Ok(name)
}

/// Derive a sort name, so that "Erik Satie" sorts as "Satie, Erik"
///
/// Compound surnames like "Ralph Vaughan Williams" come out wrong. That is acceptable because the
/// heuristic only has to be right often enough that the user rarely retypes a name.
fn sort_name(name: &str) -> String {
    let mut parts: Vec<&str> = name.split_whitespace().collect();
    match parts.pop() {
        Some(last) if !parts.is_empty() => format!("{last}, {}", parts.join(" ")),
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineOptions;
    use crate::holding::NewHolding;
    use crate::link::LinkKind;
    use crate::publication::ImportSeed;

    #[tokio::test]
    async fn links_collapse_on_record() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), EngineOptions::default())
            .await
            .unwrap();
        let library = engine.create_library("Sheet music", false).await.unwrap();
        let mut publication = library
            .create_publication(
                ImportSeed::Title("Russian piano album".into()),
                NewHolding::Physical { location: None },
            )
            .await
            .unwrap();
        let mut person = publication
            .create_contributor("Sergei Rachmaninoff", "composer")
            .await
            .unwrap();
        for url in [
            "https://www.wikidata.org/wiki/Q131861",
            "https://m.wikidata.org/wiki/Q131861",
        ] {
            person.add_link(url.parse().unwrap()).await.unwrap();
        }
        person.reload().await.unwrap();

        let [link] = person.links() else {
            panic!("expected one link, got {:?}", person.links());
        };
        assert_eq!(
            link.url.to_string(),
            "https://www.wikidata.org/wiki/Q131861"
        );
        assert_eq!(
            link.record,
            Some(ExternalRecord::new(LinkKind::Wikidata, "Q131861"))
        );
    }
}
