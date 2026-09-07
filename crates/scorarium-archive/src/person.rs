use sqlx::SqliteConnection;

use crate::Result;

/// A person who contributed to a publication or work
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Contributor {
    pub person_id: i64,
    pub name: String,
    pub role: String,
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
