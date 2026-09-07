use std::collections::HashMap;
use std::str::FromStr;

use serde::Deserialize;
use sqlx::{SqliteExecutor, SqlitePool};

use crate::db::person::{self, Contributor};
use crate::db::work;
use crate::identifier::{self, Normalized};
use crate::publication_form::PublicationUpdate;

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
                              WHERE wc.person_id = ?4)))
         ORDER BY id",
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
                              WHERE wc.person_id = ?4)))
         ORDER BY c.id",
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
                              WHERE wc.person_id = ?4)))
         ORDER BY id",
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

/// Apply a reviewed form to an existing publication and collect what the edit leaves orphaned.
pub async fn update(
    pool: &SqlitePool,
    library_id: i64,
    id: i64,
    validated: &PublicationUpdate,
) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query!(
        "UPDATE publication SET title = ?, publisher = ?, year = ? WHERE library_id = ? AND id = ?",
        validated.title,
        validated.publisher,
        validated.year,
        library_id,
        id
    )
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        tx.rollback().await?;
        return Ok(false);
    }
    write_children(&mut tx, library_id, id, validated).await?;
    // Only an edit reconciles the form's work rows against stored works; accepting an import
    // writes its works in full instead
    work::write_contents(&mut tx, library_id, id, &validated.works).await?;
    // Dropping a contributor row can leave the person behind it credited nowhere
    collect_orphans(&mut tx, library_id).await?;
    tx.commit().await?;
    Ok(true)
}

/// Write a publication's identifiers, contributor links and copies from a reviewed form.
///
/// What it leaves out is the works, since the two callers write those differently: an edit
/// reconciles the form's thin work rows against what is stored, while accepting an import creates
/// its works in full from the draft.
///
/// An identifier or a contributor link holds nothing beyond what the form shows, so both are
/// rebuilt outright. A copy is not: a row naming an existing copy updates it in place, so the copy
/// keeps its identity and whatever hangs off it. Copies the form no longer names are dropped.
///
/// On a publication with no children yet the deletes find nothing, which is what accepting an
/// import wants; one definition of how a form becomes rows then serves both.
pub async fn write_children(
    conn: &mut sqlx::SqliteConnection,
    library_id: i64,
    publication_id: i64,
    validated: &PublicationUpdate,
) -> sqlx::Result<()> {
    sqlx::query!(
        "DELETE FROM publication_identifier WHERE publication_id = ?",
        publication_id
    )
    .execute(&mut *conn)
    .await?;
    for (kind, value) in &validated.identifiers {
        create_identifier(&mut *conn, publication_id, *kind, value).await?;
    }

    sqlx::query!(
        "DELETE FROM publication_contributor WHERE publication_id = ?",
        publication_id
    )
    .execute(&mut *conn)
    .await?;
    for row in &validated.contributors {
        let person_id = person::find_or_create(&mut *conn, library_id, &row.name).await?;
        person::create_contributor(&mut *conn, library_id, publication_id, person_id, &row.role)
            .await?;
    }

    let stored = sqlx::query_scalar!(
        "SELECT id FROM holding WHERE publication_id = ?",
        publication_id
    )
    .fetch_all(&mut *conn)
    .await?;
    let named: Vec<i64> = validated.holdings.iter().filter_map(|h| h.id).collect();
    for id in stored.iter().filter(|id| !named.contains(id)) {
        sqlx::query!("DELETE FROM holding WHERE id = ?", id)
            .execute(&mut *conn)
            .await?;
    }
    for holding in &validated.holdings {
        // An id naming no copy of this publication cannot be trusted; take the row as a new copy
        match holding.id.filter(|id| stored.contains(id)) {
            Some(id) => {
                let kind = holding.kind.as_str();
                sqlx::query!(
                    "UPDATE holding SET kind = ?, location = ? WHERE id = ?",
                    kind,
                    holding.location,
                    id
                )
                .execute(&mut *conn)
                .await?;
            }
            None => {
                create_holding(
                    &mut *conn,
                    publication_id,
                    holding.kind,
                    holding.location.as_deref(),
                )
                .await?;
            }
        }
    }
    Ok(())
}

/// Delete a publication and collect what that leaves orphaned.
pub async fn delete(pool: &SqlitePool, library_id: i64, id: i64) -> sqlx::Result<bool> {
    let mut tx = pool.begin().await?;
    // Identifiers, holdings, contributor links and containment links cascade with the row
    let result = sqlx::query!(
        "DELETE FROM publication WHERE library_id = ? AND id = ?",
        library_id,
        id
    )
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        tx.rollback().await?;
        return Ok(false);
    }
    collect_orphans(&mut tx, library_id).await?;
    tx.commit().await?;
    Ok(true)
}

/// Delete what the library no longer has reachable links to.
///
/// The deletion order matters. Collecting a work takes its contributor links with it, and those
/// links can be the last thing crediting a person.
pub async fn collect_orphans(
    conn: &mut sqlx::SqliteConnection,
    library_id: i64,
) -> sqlx::Result<()> {
    sqlx::query!(
        "DELETE FROM work
         WHERE library_id = ? AND id NOT IN (SELECT work_id FROM publication_work)",
        library_id
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        "DELETE FROM person
         WHERE library_id = ?
           AND id NOT IN (SELECT person_id FROM publication_contributor)
           AND id NOT IN (SELECT person_id FROM work_contributor)",
        library_id
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::publication_form::{ContributorRow, HoldingUpdate};

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

    /// An edit keeps a copy it still names, rebuilds the rest, and collects a person it uncredits.
    #[sqlx::test]
    async fn update_reconciles_children(pool: SqlitePool) {
        let library_id = db::create_library(&pool, "lib").await.unwrap();
        let other_library = db::create_library(&pool, "other").await.unwrap();
        let publication = create_publication(
            &pool,
            &NewPublication {
                library_id,
                title: "Practial Vim",
                publisher: None,
                year: None,
            },
        )
        .await
        .unwrap();
        let untouched = create_publication(
            &pool,
            &NewPublication {
                library_id,
                title: "Untouched",
                publisher: None,
                year: None,
            },
        )
        .await
        .unwrap();
        let shelf = create_holding(&pool, publication, HoldingKind::Physical, Some("Desk"))
            .await
            .unwrap();
        create_holding(&pool, publication, HoldingKind::Digital, Some("vim.pdf"))
            .await
            .unwrap();
        let isbn = identifier::normalize(identifier::Kind::Isbn, "978-1-68050-127-8").unwrap();
        create_identifier(&pool, publication, identifier::Kind::Isbn, &isbn)
            .await
            .unwrap();
        for (name, credited_on) in [("Dropped", publication), ("Elsewhere", untouched)] {
            let person_id = db::person::create_person(&pool, library_id, name, name)
                .await
                .unwrap();
            db::person::create_contributor(&pool, library_id, credited_on, person_id, "author")
                .await
                .unwrap();
        }
        let kept = db::person::create_person(&pool, library_id, "Kept", "Kept")
            .await
            .unwrap();
        db::person::create_contributor(&pool, library_id, publication, kept, "author")
            .await
            .unwrap();

        let ismn = identifier::normalize(identifier::Kind::Ismn, "979-0-2600-0043-8").unwrap();
        let validated = PublicationUpdate {
            title: "Practical Vim".into(),
            publisher: Some("Pragmatic Bookshelf".into()),
            year: Some(2015),
            holdings: vec![
                // The shelved copy stays and moves, the pdf goes, and a copy is added
                HoldingUpdate {
                    id: Some(shelf),
                    kind: HoldingKind::Physical,
                    location: Some("Piano bench".into()),
                },
                HoldingUpdate {
                    id: None,
                    kind: HoldingKind::Digital,
                    location: Some("practical-vim.pdf".into()),
                },
            ],
            identifiers: vec![(identifier::Kind::Ismn, ismn)],
            contributors: vec![
                ContributorRow {
                    name: "Kept".into(),
                    role: "author".into(),
                },
                ContributorRow {
                    name: "Added".into(),
                    role: "editor".into(),
                },
            ],
            works: Vec::new(),
        };

        // Another library's id must not reach this publication
        assert!(
            !update(&pool, other_library, publication, &validated)
                .await
                .unwrap()
        );
        assert!(
            update(&pool, library_id, publication, &validated)
                .await
                .unwrap()
        );

        let stored = get(&pool, library_id, publication).await.unwrap().unwrap();
        assert_eq!(stored.title, "Practical Vim");
        assert_eq!(stored.publisher.as_deref(), Some("Pragmatic Bookshelf"));
        assert_eq!(stored.year, Some(2015));
        assert_eq!(
            stored
                .holdings
                .iter()
                .map(|h| (h.id == shelf, h.kind, h.location.as_deref()))
                .collect::<Vec<_>>(),
            [
                (true, HoldingKind::Physical, Some("Piano bench")),
                (false, HoldingKind::Digital, Some("practical-vim.pdf")),
            ]
        );
        assert_eq!(
            stored
                .identifiers
                .iter()
                .map(|i| (i.kind, i.value.as_str()))
                .collect::<Vec<_>>(),
            [(identifier::Kind::Ismn, "979-0-2600-0043-8")]
        );
        let persons = sqlx::query_scalar!("SELECT name FROM person ORDER BY name")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(persons, ["Added", "Elsewhere", "Kept"]);
    }

    #[sqlx::test]
    async fn delete_collects_orphans(pool: SqlitePool) {
        let library_id = db::create_library(&pool, "lib").await.unwrap();
        let other_library = db::create_library(&pool, "other").await.unwrap();
        let mut ids = Vec::new();
        for title in ["Doomed", "Keeper"] {
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
        let [doomed, keeper] = ids[..] else {
            unreachable!()
        };
        let mut ids = Vec::new();
        for title in ["Shared", "Doomed only"] {
            let id = db::work::create_work(
                &pool,
                &db::work::NewWork {
                    library_id,
                    title,
                    key: None,
                    time_signature: None,
                    instrumentation: None,
                },
            )
            .await
            .unwrap();
            ids.push(id);
        }
        let [shared, doomed_only] = ids[..] else {
            unreachable!()
        };
        db::work::create_catalog_number(&pool, shared, "S 1")
            .await
            .unwrap();
        db::work::create_catalog_number(&pool, doomed_only, "D 1")
            .await
            .unwrap();
        for (publication, work) in [(doomed, shared), (keeper, shared), (doomed, doomed_only)] {
            db::work::add_to_publication(&pool, library_id, publication, work)
                .await
                .unwrap();
        }

        let mut ids = Vec::new();
        for name in ["Direct", "Via work", "Elsewhere", "On shared"] {
            let id = db::person::create_person(&pool, library_id, name, name)
                .await
                .unwrap();
            ids.push(id);
        }
        let [direct, via_work, elsewhere, on_shared] = ids[..] else {
            unreachable!()
        };
        db::person::create_contributor(&pool, library_id, doomed, direct, "editor")
            .await
            .unwrap();
        db::person::create_contributor(&pool, library_id, keeper, elsewhere, "editor")
            .await
            .unwrap();
        db::work::create_contributor(&pool, library_id, doomed_only, via_work, "composer")
            .await
            .unwrap();
        db::work::create_contributor(&pool, library_id, shared, on_shared, "composer")
            .await
            .unwrap();

        // Another library's id must not reach this publication
        assert!(!delete(&pool, other_library, doomed).await.unwrap());
        assert!(get(&pool, library_id, doomed).await.unwrap().is_some());

        assert!(delete(&pool, library_id, doomed).await.unwrap());

        let titles: Vec<String> = list(&pool, library_id)
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.title)
            .collect();
        assert_eq!(titles, ["Keeper"]);
        let works = sqlx::query_scalar!("SELECT title FROM work ORDER BY title")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(works, ["Shared"]);
        let catalog_numbers =
            sqlx::query_scalar!("SELECT value FROM work_catalog_number ORDER BY value")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(catalog_numbers, ["S 1"]);
        let persons = sqlx::query_scalar!("SELECT name FROM person ORDER BY name")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(persons, ["Elsewhere", "On shared"]);
    }
}
