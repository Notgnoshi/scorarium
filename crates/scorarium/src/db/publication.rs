use std::str::FromStr;

use sqlx::SqlitePool;

use crate::identifier::{self, Normalized};

pub struct NewPublication<'a> {
    pub library_id: i64,
    pub title: &'a str,
    pub publisher: Option<&'a str>,
    pub year: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoldingKind {
    Physical,
    Digital,
}

impl HoldingKind {
    pub fn as_str(self) -> &'static str {
        match self {
            HoldingKind::Physical => "physical",
            HoldingKind::Digital => "digital",
        }
    }
}

impl FromStr for HoldingKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "physical" => Ok(HoldingKind::Physical),
            "digital" => Ok(HoldingKind::Digital),
            _ => Err(format!("unknown holding kind: {s}")),
        }
    }
}

pub async fn create_publication(pool: &SqlitePool, new: &NewPublication<'_>) -> sqlx::Result<i64> {
    let result = sqlx::query!(
        "INSERT INTO publication (library_id, title, publisher, year) VALUES (?, ?, ?, ?)",
        new.library_id,
        new.title,
        new.publisher,
        new.year,
    )
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn create_holding(
    pool: &SqlitePool,
    publication_id: i64,
    kind: HoldingKind,
    location: Option<&str>,
) -> sqlx::Result<i64> {
    let kind = kind.as_str();
    let result = sqlx::query!(
        "INSERT INTO holding (publication_id, kind, location) VALUES (?, ?, ?)",
        publication_id,
        kind,
        location,
    )
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn create_identifier(
    pool: &SqlitePool,
    publication_id: i64,
    kind: identifier::Kind,
    value: &Normalized,
) -> sqlx::Result<i64> {
    let kind = kind.as_str();
    let value = value.as_str();
    let result = sqlx::query!(
        "INSERT INTO publication_identifier (publication_id, kind, value) VALUES (?, ?, ?)",
        publication_id,
        kind,
        value,
    )
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    /// Deleting a library must take its publications and their children with it.
    #[sqlx::test]
    async fn delete_library_cascades(pool: SqlitePool) {
        let library_id = db::create_library(&pool, "lib").await.unwrap();
        let new = NewPublication {
            library_id,
            title: "Practical Vim",
            publisher: Some("Pragmatic Bookshelf"),
            year: Some(2015),
        };
        let publication_id = create_publication(&pool, &new).await.unwrap();
        create_holding(&pool, publication_id, HoldingKind::Physical, Some("Desk"))
            .await
            .unwrap();
        let isbn = identifier::normalize(identifier::Kind::Isbn, "978-1-68050-127-8").unwrap();
        create_identifier(&pool, publication_id, identifier::Kind::Isbn, &isbn)
            .await
            .unwrap();
        let person_id = db::person::create_person(&pool, library_id, "Drew Neil", "Neil, Drew")
            .await
            .unwrap();
        db::person::create_contributor(&pool, publication_id, person_id, "author")
            .await
            .unwrap();

        assert!(db::delete_library(&pool, library_id).await.unwrap());

        let holdings = sqlx::query_scalar!("SELECT COUNT(*) FROM holding")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(holdings, 0);
        let identifiers = sqlx::query_scalar!("SELECT COUNT(*) FROM publication_identifier")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(identifiers, 0);
        let persons = sqlx::query_scalar!("SELECT COUNT(*) FROM person")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(persons, 0);
        let contributors = sqlx::query_scalar!("SELECT COUNT(*) FROM publication_contributor")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(contributors, 0);
    }
}
