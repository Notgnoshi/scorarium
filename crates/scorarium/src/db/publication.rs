use std::collections::HashMap;
use std::str::FromStr;

use serde::Deserialize;
use sqlx::{SqliteExecutor, SqlitePool};

use crate::db::person::Contributor;
use crate::identifier::{self, Normalized};

/// A publication with its children, as read back. Pages pick the fields they show.
#[derive(Debug, PartialEq, Eq)]
pub struct Publication {
    pub id: i64,
    pub library_id: i64,
    pub title: String,
    pub publisher: Option<String>,
    pub year: Option<i64>,
    pub identifiers: Vec<Identifier>,
    pub contributors: Vec<Contributor>,
    pub holdings: Vec<Holding>,
}

impl Publication {
    /// The roles one person is credited with, for pages about that person.
    pub fn roles_of(&self, person_id: i64) -> Vec<&str> {
        self.contributors
            .iter()
            .filter(|c| c.person_id == person_id)
            .map(|c| c.role.as_str())
            .collect()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Identifier {
    pub id: i64,
    pub kind: identifier::Kind,
    pub value: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Holding {
    pub id: i64,
    pub kind: HoldingKind,
    pub location: Option<String>,
}

pub struct NewPublication<'a> {
    pub library_id: i64,
    pub title: &'a str,
    pub publisher: Option<&'a str>,
    pub year: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
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

pub fn decode_error(message: String) -> sqlx::Error {
    sqlx::Error::Decode(message.into())
}

/// All publications in a library, with their children, in arbitrary order.
pub async fn list(pool: &SqlitePool, library_id: i64) -> sqlx::Result<Vec<Publication>> {
    load(pool, library_id, None, None, None).await
}

/// One publication, or None when it does not exist or belongs to another library.
pub async fn get(pool: &SqlitePool, library_id: i64, id: i64) -> sqlx::Result<Option<Publication>> {
    Ok(load(pool, library_id, Some(id), None, None).await?.pop())
}

/// The publications containing a work, with their children, in arbitrary order.
pub async fn list_containing(
    pool: &SqlitePool,
    library_id: i64,
    work_id: i64,
) -> sqlx::Result<Vec<Publication>> {
    load(pool, library_id, None, Some(work_id), None).await
}

/// The publications a person is credited on, directly or through a contained work, with their
/// children, in arbitrary order.
pub async fn list_by_person(
    pool: &SqlitePool,
    library_id: i64,
    person_id: i64,
) -> sqlx::Result<Vec<Publication>> {
    load(pool, library_id, None, None, Some(person_id)).await
}

/// Load a library's publications with their children: all of them, just the one with `id`, those
/// containing `work_id`, or those crediting `person_id` directly or through a contained work.
///
/// The four reads share a transaction so they see one snapshot. Otherwise a child row for a
/// publication created between the parent read and the child reads would have no parent here.
async fn load(
    pool: &SqlitePool,
    library_id: i64,
    id: Option<i64>,
    work_id: Option<i64>,
    person_id: Option<i64>,
) -> sqlx::Result<Vec<Publication>> {
    let mut tx = pool.begin().await?;
    let mut publications: Vec<Publication> = sqlx::query!(
        "SELECT id, library_id, title, publisher, year FROM publication
         WHERE library_id = ?1
           AND (?2 IS NULL OR id = ?2)
           AND (?3 IS NULL OR id IN (SELECT publication_id FROM publication_work WHERE work_id = ?3))
           AND (?4 IS NULL
                OR id IN (SELECT publication_id FROM publication_contributor WHERE person_id = ?4)
                OR id IN (SELECT pw.publication_id FROM publication_work pw
                          JOIN work_contributor wc ON wc.work_id = pw.work_id
                          WHERE wc.person_id = ?4))",
        library_id,
        id,
        work_id,
        person_id
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|row| Publication {
        id: row.id,
        library_id: row.library_id,
        title: row.title,
        publisher: row.publisher,
        year: row.year,
        identifiers: Vec::new(),
        contributors: Vec::new(),
        holdings: Vec::new(),
    })
    .collect();
    let index: HashMap<i64, usize> = publications
        .iter()
        .enumerate()
        .map(|(i, p)| (p.id, i))
        .collect();

    let identifiers = sqlx::query!(
        "SELECT id, publication_id, kind, value FROM publication_identifier
         WHERE publication_id IN
            (SELECT id FROM publication
             WHERE library_id = ?1
               AND (?2 IS NULL OR id = ?2)
               AND (?3 IS NULL OR id IN (SELECT publication_id FROM publication_work WHERE work_id = ?3))
               AND (?4 IS NULL
                    OR id IN (SELECT publication_id FROM publication_contributor WHERE person_id = ?4)
                    OR id IN (SELECT pw.publication_id FROM publication_work pw
                              JOIN work_contributor wc ON wc.work_id = pw.work_id
                              WHERE wc.person_id = ?4)))",
        library_id,
        id,
        work_id,
        person_id
    )
    .fetch_all(&mut *tx)
    .await?;
    for row in identifiers {
        let kind = row.kind.parse().map_err(decode_error)?;
        publications[index[&row.publication_id]]
            .identifiers
            .push(Identifier {
                id: row.id,
                kind,
                value: row.value,
            });
    }

    let contributors = sqlx::query!(
        "SELECT c.publication_id, c.person_id, p.name, c.role
         FROM publication_contributor c JOIN person p ON p.id = c.person_id
         WHERE c.publication_id IN
            (SELECT id FROM publication
             WHERE library_id = ?1
               AND (?2 IS NULL OR id = ?2)
               AND (?3 IS NULL OR id IN (SELECT publication_id FROM publication_work WHERE work_id = ?3))
               AND (?4 IS NULL
                    OR id IN (SELECT publication_id FROM publication_contributor WHERE person_id = ?4)
                    OR id IN (SELECT pw.publication_id FROM publication_work pw
                              JOIN work_contributor wc ON wc.work_id = pw.work_id
                              WHERE wc.person_id = ?4)))",
        library_id,
        id,
        work_id,
        person_id
    )
    .fetch_all(&mut *tx)
    .await?;
    for row in contributors {
        publications[index[&row.publication_id]]
            .contributors
            .push(Contributor {
                person_id: row.person_id,
                name: row.name,
                role: row.role,
            });
    }

    let holdings = sqlx::query!(
        "SELECT id, publication_id, kind, location FROM holding
         WHERE publication_id IN
            (SELECT id FROM publication
             WHERE library_id = ?1
               AND (?2 IS NULL OR id = ?2)
               AND (?3 IS NULL OR id IN (SELECT publication_id FROM publication_work WHERE work_id = ?3))
               AND (?4 IS NULL
                    OR id IN (SELECT publication_id FROM publication_contributor WHERE person_id = ?4)
                    OR id IN (SELECT pw.publication_id FROM publication_work pw
                              JOIN work_contributor wc ON wc.work_id = pw.work_id
                              WHERE wc.person_id = ?4)))",
        library_id,
        id,
        work_id,
        person_id
    )
    .fetch_all(&mut *tx)
    .await?;
    for row in holdings {
        let kind = row.kind.parse().map_err(decode_error)?;
        publications[index[&row.publication_id]]
            .holdings
            .push(Holding {
                id: row.id,
                kind,
                location: row.location,
            });
    }

    tx.commit().await?;
    Ok(publications)
}

pub async fn create_publication(
    executor: impl SqliteExecutor<'_>,
    new: &NewPublication<'_>,
) -> sqlx::Result<i64> {
    let result = sqlx::query!(
        "INSERT INTO publication (library_id, title, publisher, year) VALUES (?, ?, ?, ?)",
        new.library_id,
        new.title,
        new.publisher,
        new.year,
    )
    .execute(executor)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn create_holding(
    executor: impl SqliteExecutor<'_>,
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
    .execute(executor)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn create_identifier(
    executor: impl SqliteExecutor<'_>,
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
    .execute(executor)
    .await?;
    Ok(result.last_insert_rowid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[sqlx::test]
    async fn list_assembles_children(pool: SqlitePool) {
        let library_id = db::create_library(&pool, "lib").await.unwrap();
        let pro_git = create_publication(
            &pool,
            &NewPublication {
                library_id,
                title: "Pro Git",
                publisher: Some("Apress"),
                year: Some(2014),
            },
        )
        .await
        .unwrap();
        let bare = create_publication(
            &pool,
            &NewPublication {
                library_id,
                title: "Bare",
                publisher: None,
                year: None,
            },
        )
        .await
        .unwrap();
        let chacon = db::person::create_person(&pool, library_id, "Scott Chacon", "Chacon, Scott")
            .await
            .unwrap();
        let straub = db::person::create_person(&pool, library_id, "Ben Straub", "Straub, Ben")
            .await
            .unwrap();
        db::person::create_contributor(&pool, library_id, pro_git, chacon, "author")
            .await
            .unwrap();
        db::person::create_contributor(&pool, library_id, pro_git, straub, "author")
            .await
            .unwrap();
        let isbn = identifier::normalize(identifier::Kind::Isbn, "978-1-4842-0077-3").unwrap();
        let isbn_id = create_identifier(&pool, pro_git, identifier::Kind::Isbn, &isbn)
            .await
            .unwrap();
        let shelf = create_holding(&pool, pro_git, HoldingKind::Physical, None)
            .await
            .unwrap();
        let pdf = create_holding(&pool, pro_git, HoldingKind::Digital, Some("pro-git.pdf"))
            .await
            .unwrap();

        let mut publications = list(&pool, library_id).await.unwrap();
        publications.sort_by_key(|p| p.id);
        assert_eq!(
            publications,
            [
                Publication {
                    id: pro_git,
                    library_id,
                    title: "Pro Git".into(),
                    publisher: Some("Apress".into()),
                    year: Some(2014),
                    identifiers: vec![Identifier {
                        id: isbn_id,
                        kind: identifier::Kind::Isbn,
                        value: "978-1-4842-0077-3".into(),
                    }],
                    contributors: vec![
                        Contributor {
                            person_id: chacon,
                            name: "Scott Chacon".into(),
                            role: "author".into(),
                        },
                        Contributor {
                            person_id: straub,
                            name: "Ben Straub".into(),
                            role: "author".into(),
                        },
                    ],
                    holdings: vec![
                        Holding {
                            id: shelf,
                            kind: HoldingKind::Physical,
                            location: None,
                        },
                        Holding {
                            id: pdf,
                            kind: HoldingKind::Digital,
                            location: Some("pro-git.pdf".into()),
                        },
                    ],
                },
                Publication {
                    id: bare,
                    library_id,
                    title: "Bare".into(),
                    publisher: None,
                    year: None,
                    identifiers: vec![],
                    contributors: vec![],
                    holdings: vec![],
                },
            ]
        );

        assert_eq!(
            get(&pool, library_id, pro_git).await.unwrap().unwrap().id,
            pro_git
        );
        let other_library = db::create_library(&pool, "other").await.unwrap();
        assert_eq!(get(&pool, other_library, pro_git).await.unwrap(), None);
    }

    /// A person's publications are those crediting them directly and those containing a work that
    /// credits them.
    #[sqlx::test]
    async fn list_by_person_unions_credits(pool: SqlitePool) {
        let library_id = db::create_library(&pool, "lib").await.unwrap();
        let person = db::person::create_person(&pool, library_id, "Erik Satie", "Satie, Erik")
            .await
            .unwrap();
        let other = db::person::create_person(&pool, library_id, "Other", "Other")
            .await
            .unwrap();
        let mut ids = Vec::new();
        for title in ["Edited", "Contains work", "Unrelated"] {
            let id = create_publication(
                &pool,
                &NewPublication {
                    library_id,
                    title,
                    publisher: None,
                    year: None,
                },
            )
            .await
            .unwrap();
            ids.push(id);
        }
        let [edited, contains_work, unrelated] = ids[..] else {
            unreachable!()
        };
        db::person::create_contributor(&pool, library_id, edited, person, "editor")
            .await
            .unwrap();
        db::person::create_contributor(&pool, library_id, unrelated, other, "composer")
            .await
            .unwrap();
        for (publication, composer) in [(contains_work, person), (unrelated, other)] {
            let work = db::work::create_work(
                &pool,
                &db::work::NewWork {
                    library_id,
                    title: "Piece",
                    key: None,
                    time_signature: None,
                    instrumentation: None,
                },
            )
            .await
            .unwrap();
            db::work::add_to_publication(&pool, library_id, publication, work)
                .await
                .unwrap();
            db::work::create_contributor(&pool, library_id, work, composer, "composer")
                .await
                .unwrap();
        }

        let mut publications = list_by_person(&pool, library_id, person).await.unwrap();
        publications.sort_by_key(|p| p.id);
        let found: Vec<i64> = publications.iter().map(|p| p.id).collect();
        assert_eq!(found, [edited, contains_work]);
        assert_eq!(publications[0].roles_of(person), ["editor"]);
        assert_eq!(publications[1].roles_of(person), [] as [&str; 0]);
    }

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
        db::person::create_contributor(&pool, library_id, publication_id, person_id, "author")
            .await
            .unwrap();
        let work_id = db::work::create_work(
            &pool,
            &db::work::NewWork {
                library_id,
                title: "Chapter 1",
                key: None,
                time_signature: None,
                instrumentation: None,
            },
        )
        .await
        .unwrap();
        db::work::create_catalog_number(&pool, work_id, "Ch. 1")
            .await
            .unwrap();
        db::work::add_to_publication(&pool, library_id, publication_id, work_id)
            .await
            .unwrap();
        db::work::create_contributor(&pool, library_id, work_id, person_id, "author")
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
        let works = sqlx::query_scalar!("SELECT COUNT(*) FROM work")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(works, 0);
        let catalog_numbers = sqlx::query_scalar!("SELECT COUNT(*) FROM work_catalog_number")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(catalog_numbers, 0);
        let publication_works = sqlx::query_scalar!("SELECT COUNT(*) FROM publication_work")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(publication_works, 0);
        let work_contributors = sqlx::query_scalar!("SELECT COUNT(*) FROM work_contributor")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(work_contributors, 0);
    }
}
