use std::collections::HashMap;
use std::sync::Arc;

use comparable::{Changed, Comparable};
use sqlx::SqliteConnection;

use crate::fuzzy::normalize;
use crate::input::{self, ContributorInput, PersonRef, ValidationError};
use crate::publication::{self, Publication};
use crate::{Action, ArchiveInner, EntityKind, EntityRef, Event, Field, NotFound, Result, Source};

/// A person who contributed to a publication or work
///
/// A [Contributor] is tied to a particular publication or work, but it's really a [Person] that
/// with an associated role.
#[derive(Clone, Debug, PartialEq, Eq, Comparable)]
pub struct Contributor {
    pub person_id: i64,
    pub name: String,
    pub role: String,
}

/// A person's editable fields as entered from the web form
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PersonRawInput {
    pub name: String,
    pub links: Vec<String>,
}

/// A person's parsed and validated fields
#[derive(Debug, PartialEq, Eq)]
pub struct PersonInput {
    pub(crate) name: String,
    pub(crate) links: Vec<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PersonErrors {
    pub name: Option<ValidationError>,
    /// One slot per link, empty when they all passed
    pub links: Vec<Option<ValidationError>>,
}

impl PersonErrors {
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.links.iter().all(Option::is_none)
    }
}

impl PersonRawInput {
    /// Check and convert the input
    ///
    /// The sort name is not input: it is derived from the name.
    pub fn parse(&self) -> std::result::Result<PersonInput, PersonErrors> {
        let name = self.name.trim();
        let mut errors = PersonErrors {
            name: name.is_empty().then_some(ValidationError::NameRequired),
            links: Vec::new(),
        };
        let links = match input::parse_links(&self.links) {
            Ok(links) => links,
            Err(slots) => {
                errors.links = slots;
                Vec::new()
            }
        };
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(PersonInput {
            name: name.to_string(),
            links,
        })
    }
}

/// Someone credited somewhere in a library
///
/// A [Contributor] is a [Person] that contributed to a publication or work with a specific role.
#[derive(Clone, Debug, Comparable)]
pub struct Person {
    #[comparable_ignore]
    pub id: i64,
    #[comparable_ignore]
    pub library_id: i64,
    pub name: String,
    /// How the name should get sorted: "Satie, Erik" for "Erik Satie"
    #[comparable_ignore]
    pub sort_name: String,
    /// In the order they were entered
    pub links: Vec<String>,
    #[comparable_ignore]
    archive: Arc<ArchiveInner>,
}

/// Which fields an edit changed, for the audit log.
fn changed_fields(old: &Person, new: &Person) -> Vec<Field> {
    let Changed::Changed(changes) = old.comparison(new) else {
        return Vec::new();
    };
    changes
        .iter()
        .map(|change| match change {
            PersonChange::Name(_) => Field::Name,
            PersonChange::Links(_) => Field::Links,
        })
        .collect()
}

impl Person {
    /// The publications crediting this person, whether directly on a publication or indirectly
    /// through a work in a publication.
    pub async fn publications(&self) -> Result<Vec<Publication>> {
        let mut tx = self.archive.begin_read().await?;
        let publications = publication::load_publications(
            &self.archive,
            &mut tx,
            self.library_id,
            None,
            None,
            Some(self.id),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(publications)
    }

    /// What the person's edit page opens with
    pub fn raw_input(&self) -> PersonRawInput {
        PersonRawInput {
            name: self.name.clone(),
            links: self.links.clone(),
        }
    }

    /// Apply an edited input.
    ///
    /// Returns a [NotFound] error if the person has since been collected.
    pub async fn update(&mut self, input: &PersonInput) -> Result<()> {
        let mut audited = self
            .archive
            .begin_audit(
                Source::User,
                Event::about(Action::Updated, self.entity_ref()),
            )
            .await?;
        let sort_name = sort_name(&input.name);
        let result = sqlx::query!(
            "UPDATE person SET name = ?, sort_name = ? WHERE library_id = ? AND id = ?",
            input.name,
            sort_name,
            self.library_id,
            self.id
        )
        .execute(&mut *audited)
        .await?;
        if result.rows_affected() == 0 {
            audited.rollback().await?;
            return Err(NotFound.into());
        }
        write_person_links(&mut audited, self.id, &input.links).await?;
        let reloaded = load_persons(
            &self.archive,
            &mut audited,
            self.library_id,
            Some(self.id),
            None,
        )
        .await?
        .pop()
        .expect("the person was just updated on this transaction");
        audited.set_fields(&changed_fields(self, &reloaded)).await?;
        audited.commit().await?;
        *self = reloaded;
        Ok(())
    }

    /// Get an EntityRef referring to this entity for use in the audit log
    pub(crate) fn entity_ref(&self) -> EntityRef {
        EntityRef {
            kind: EntityKind::Person,
            id: self.id,
            library_id: Some(self.library_id),
            label: self.name.clone(),
        }
    }
}

pub(crate) async fn load_persons(
    shared: &Arc<ArchiveInner>,
    conn: &mut SqliteConnection,
    library_id: i64,
    id: Option<i64>,
    role: Option<&str>,
) -> Result<Vec<Person>> {
    let mut persons: Vec<Person> = sqlx::query!(
        "SELECT id, library_id, name, sort_name FROM person
         WHERE library_id = ?1
           AND (?2 IS NULL OR id = ?2)
           AND (?3 IS NULL
                OR id IN (SELECT person_id FROM publication_contributor WHERE role = ?3)
                OR id IN (SELECT person_id FROM work_contributor WHERE role = ?3))
         ORDER BY sort_name",
        library_id,
        id,
        role
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| Person {
        id: row.id,
        library_id: row.library_id,
        name: row.name,
        sort_name: row.sort_name,
        links: Vec::new(),
        archive: shared.clone(),
    })
    .collect();
    let index: HashMap<i64, usize> = persons
        .iter()
        .enumerate()
        .map(|(i, person)| (person.id, i))
        .collect();

    let links = sqlx::query!(
        "SELECT person_id, url FROM person_link
         WHERE person_id IN
            (SELECT id FROM person
             WHERE library_id = ?1
               AND (?2 IS NULL OR id = ?2)
               AND (?3 IS NULL
                    OR id IN (SELECT person_id FROM publication_contributor WHERE role = ?3)
                    OR id IN (SELECT person_id FROM work_contributor WHERE role = ?3)))
         ORDER BY id",
        library_id,
        id,
        role
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in links {
        persons[index[&row.person_id]].links.push(row.url);
    }

    Ok(persons)
}

async fn write_person_links(
    conn: &mut SqliteConnection,
    person_id: i64,
    links: &[String],
) -> Result<()> {
    sqlx::query!("DELETE FROM person_link WHERE person_id = ?", person_id)
        .execute(&mut *conn)
        .await?;
    for url in links {
        sqlx::query!(
            "INSERT INTO person_link (person_id, url) VALUES (?, ?)",
            person_id,
            url
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Create the persons a submission asks for, one per distinct normalized name, and link each
/// contributor asking for one to it.
pub(crate) async fn create_new_persons<'a>(
    conn: &mut SqliteConnection,
    library_id: i64,
    contributors: impl Iterator<Item = &'a mut ContributorInput>,
) -> Result<()> {
    let mut created: HashMap<String, i64> = HashMap::new();
    for contributor in contributors {
        if contributor.person != PersonRef::New {
            continue;
        }
        let id = match created.get(&normalize(&contributor.name)) {
            Some(id) => *id,
            None => {
                let id = create_person(&mut *conn, library_id, &contributor.name).await?;
                created.insert(normalize(&contributor.name), id);
                id
            }
        };
        contributor.person = PersonRef::Linked(id);
    }
    Ok(())
}

/// The person a contributor credits
///
/// A linked person can be collected between the page loading and its submit, in which case the
/// typed name becomes a new person rather than losing the credit.
pub(crate) async fn credited_person(
    conn: &mut SqliteConnection,
    library_id: i64,
    contributor: &ContributorInput,
) -> Result<i64> {
    match contributor.person {
        PersonRef::Linked(id) => {
            let found = sqlx::query_scalar!(
                "SELECT id FROM person WHERE library_id = ? AND id = ?",
                library_id,
                id
            )
            .fetch_optional(&mut *conn)
            .await?;
            match found {
                Some(id) => Ok(id),
                None => create_person(conn, library_id, &contributor.name).await,
            }
        }
        PersonRef::New => create_person(conn, library_id, &contributor.name).await,
        // The parser refuses these, and the demo resolves its own, so one here is a bug
        PersonRef::Unresolved => Err(eyre::eyre!(
            "an unresolved contributor reached the write path: {:?}",
            contributor.name
        )),
    }
}

/// Create a person, whether or not the library has one by that name
async fn create_person(conn: &mut SqliteConnection, library_id: i64, name: &str) -> Result<i64> {
    let sort_name = sort_name(name);
    let created = sqlx::query!(
        "INSERT INTO person (library_id, name, sort_name) VALUES (?, ?, ?)",
        library_id,
        name,
        sort_name,
    )
    .execute(conn)
    .await?;
    Ok(created.last_insert_rowid())
}

/// How strongly a role identifies the work it is credited on, lowest first.
///
/// A composer names a piece of music and an author names a piece of prose; everyone else helped
/// with one. This is what picks the single contributor a work shows wherever it is listed.
pub fn credit_priority(role: &str) -> u8 {
    match role {
        "composer" => 0,
        "author" => 1,
        _ => 2,
    }
}

/// The distinct roles credited anywhere in the library, sorted
pub(crate) async fn list_contributor_roles(
    conn: &mut SqliteConnection,
    library_id: i64,
) -> Result<Vec<String>> {
    let roles = sqlx::query_scalar!(
        "SELECT role FROM publication_contributor WHERE library_id = ?1
         UNION
         SELECT role FROM work_contributor WHERE library_id = ?1
         ORDER BY role",
        library_id
    )
    .fetch_all(conn)
    .await?;
    Ok(roles)
}

/// Every person's display name in the library, by sort name
pub(crate) async fn list_person_names(
    conn: &mut SqliteConnection,
    library_id: i64,
) -> Result<Vec<String>> {
    let names = sqlx::query_scalar!(
        "SELECT name FROM person WHERE library_id = ? ORDER BY sort_name",
        library_id
    )
    .fetch_all(conn)
    .await?;
    Ok(names)
}

/// "Erik Satie" sorts as "Satie, Erik"
///
/// Compound surnames like "Ralph Vaughan Williams" come out wrong; that's acceptable for now. The
/// heuristic only has to be right often enough that the user rarely types a name twice.
fn sort_name(name: &str) -> String {
    let mut parts: Vec<&str> = name.split_whitespace().collect();
    match parts.pop() {
        Some(last) if !parts.is_empty() => format!("{last}, {}", parts.join(" ")),
        _ => name.to_string(),
    }
}
