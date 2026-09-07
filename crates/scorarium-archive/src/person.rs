use std::sync::Arc;

use sqlx::SqliteConnection;

use crate::publication::{self, Publication};
use crate::{ArchiveInner, Result};

/// A person who contributed to a publication or work
///
/// A [Contributor] is tied to a particular publication or work, but it's really a [Person] that
/// with an associated role.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contributor {
    pub person_id: i64,
    pub name: String,
    pub role: String,
}

/// Someone credited somewhere in a library
///
/// A [Contributor] is a [Person] that contributed to a publication or work with a specific role.
#[derive(Clone, Debug)]
pub struct Person {
    pub id: i64,
    pub library_id: i64,
    pub name: String,
    /// How the name should get sorted: "Satie, Erik" for "Erik Satie"
    pub sort_name: String,
    archive: Arc<ArchiveInner>,
}

impl Person {
    /// The publications crediting this person, whether directly on a publication or indirectly
    /// through a work in a publication.
    pub async fn publications(&self) -> Result<Vec<Publication>> {
        let mut tx = self.archive.pool.begin().await?;
        let publications = publication::load_publications(
            &self.archive,
            &mut tx,
            self.library_id,
            None,
            None,
            Some(self.id),
        )
        .await?;
        tx.commit().await?;
        Ok(publications)
    }
}

pub(crate) async fn get_person(
    shared: &Arc<ArchiveInner>,
    conn: &mut SqliteConnection,
    library_id: i64,
    id: i64,
) -> Result<Option<Person>> {
    let row = sqlx::query!(
        "SELECT id, library_id, name, sort_name FROM person WHERE library_id = ? AND id = ?",
        library_id,
        id
    )
    .fetch_optional(conn)
    .await?;
    Ok(row.map(|row| Person {
        id: row.id,
        library_id: row.library_id,
        name: row.name,
        sort_name: row.sort_name,
        archive: shared.clone(),
    }))
}

/// Everyone credited with `role` on any publication or work in the library, by sort name
pub(crate) async fn list_persons_with_role(
    shared: &Arc<ArchiveInner>,
    conn: &mut SqliteConnection,
    library_id: i64,
    role: &str,
) -> Result<Vec<Person>> {
    let rows = sqlx::query!(
        "SELECT id, library_id, name, sort_name FROM person
         WHERE library_id = ?1
           AND (id IN (SELECT person_id FROM publication_contributor WHERE role = ?2)
                OR id IN (SELECT person_id FROM work_contributor WHERE role = ?2))
         ORDER BY sort_name",
        library_id,
        role
    )
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| Person {
            id: row.id,
            library_id: row.library_id,
            name: row.name,
            sort_name: row.sort_name,
            archive: shared.clone(),
        })
        .collect())
}

/// The person with this exact name, created if the library has none
///
/// A person created for an earlier contributor of the same publication is found by a later one,
/// because they share the caller's transaction.
pub(crate) async fn find_or_create_person(
    conn: &mut SqliteConnection,
    library_id: i64,
    name: &str,
) -> Result<i64> {
    let found = sqlx::query_scalar!(
        "SELECT id FROM person WHERE library_id = ? AND name = ?",
        library_id,
        name
    )
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(id) = found {
        return Ok(id);
    }
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
