use std::collections::HashMap;

use sqlx::SqliteConnection;

use crate::Result;
use crate::catalog::CatalogNumber;
use crate::holding::HoldingKind;
use crate::person::{Contributor, credit_priority};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicationSummary {
    pub id: i64,
    pub title: String,
    /// Contributor names in credit order
    pub people: Vec<String>,
    pub year: Option<i64>,
    pub holdings: Vec<HoldingKind>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkSummary {
    pub id: i64,
    pub title: String,
    /// The lead contributor on the work
    pub contributor: Option<Contributor>,
    /// Highest-priority scheme first, then entry order
    pub numbers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonSummary {
    pub id: i64,
    pub name: String,
    /// Distinct works credited in any role. A publication with no works counts as a work
    pub works: i64,
}

/// Where a summary came from, since search spans libraries
pub(crate) struct InLibrary<T> {
    pub library_id: i64,
    pub library_name: String,
    pub summary: T,
}

pub(crate) async fn publications(
    conn: &mut SqliteConnection,
    library: Option<i64>,
    public_only: bool,
) -> Result<Vec<InLibrary<PublicationSummary>>> {
    let mut publications: Vec<InLibrary<PublicationSummary>> = sqlx::query!(
        r#"SELECT p.id, p.title, p.year, l.id AS library_id, l.name AS library_name
           FROM publication p JOIN library l ON l.id = p.library_id
           WHERE (?1 IS NULL OR p.library_id = ?1) AND (NOT ?2 OR l.private = 0)
           ORDER BY l.name, p.title"#,
        library,
        public_only
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| InLibrary {
        library_id: row.library_id,
        library_name: row.library_name,
        summary: PublicationSummary {
            id: row.id,
            title: row.title,
            people: Vec::new(),
            year: row.year,
            holdings: Vec::new(),
        },
    })
    .collect();
    let index = index_of(&publications, |summary| summary.id);

    let people = sqlx::query!(
        r#"SELECT c.publication_id, per.name
           FROM publication_contributor c JOIN person per ON per.id = c.person_id
           JOIN publication p ON p.id = c.publication_id JOIN library l ON l.id = p.library_id
           WHERE (?1 IS NULL OR p.library_id = ?1) AND (NOT ?2 OR l.private = 0)
           ORDER BY c.id"#,
        library,
        public_only
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in people {
        publications[index[&row.publication_id]]
            .summary
            .people
            .push(row.name);
    }

    let holdings = sqlx::query!(
        r#"SELECT h.publication_id, h.kind
           FROM holding h JOIN publication p ON p.id = h.publication_id
           JOIN library l ON l.id = p.library_id
           WHERE (?1 IS NULL OR p.library_id = ?1) AND (NOT ?2 OR l.private = 0)
           ORDER BY h.id"#,
        library,
        public_only
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in holdings {
        let kind = row.kind.parse().map_err(eyre::Report::msg)?;
        publications[index[&row.publication_id]]
            .summary
            .holdings
            .push(kind);
    }
    Ok(publications)
}

pub(crate) async fn works(
    conn: &mut SqliteConnection,
    library: Option<i64>,
    public_only: bool,
    credited_to: Option<i64>,
) -> Result<Vec<InLibrary<WorkSummary>>> {
    let mut works: Vec<InLibrary<WorkSummary>> = sqlx::query!(
        r#"SELECT w.id, w.title, l.id AS library_id, l.name AS library_name
           FROM work w JOIN library l ON l.id = w.library_id
           WHERE (?1 IS NULL OR w.library_id = ?1) AND (NOT ?2 OR l.private = 0)
             AND (?3 IS NULL OR w.id IN (SELECT work_id FROM work_contributor WHERE person_id = ?3))
           ORDER BY l.name, w.title"#,
        library,
        public_only,
        credited_to
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| InLibrary {
        library_id: row.library_id,
        library_name: row.library_name,
        summary: WorkSummary {
            id: row.id,
            title: row.title,
            contributor: None,
            numbers: Vec::new(),
        },
    })
    .collect();
    let index = index_of(&works, |summary| summary.id);

    let contributors = sqlx::query!(
        r#"SELECT c.work_id, c.person_id, per.name, c.role
           FROM work_contributor c JOIN person per ON per.id = c.person_id
           JOIN work w ON w.id = c.work_id JOIN library l ON l.id = w.library_id
           WHERE (?1 IS NULL OR w.library_id = ?1) AND (NOT ?2 OR l.private = 0)
             AND (?3 IS NULL OR w.id IN (SELECT work_id FROM work_contributor WHERE person_id = ?3))
           ORDER BY c.id"#,
        library,
        public_only,
        credited_to
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in contributors {
        // Rows arrive in credit order, so keeping the first of equal priority picks the lead
        let credited = &mut works[index[&row.work_id]].summary.contributor;
        let leads = credited
            .as_ref()
            .is_none_or(|current| credit_priority(&row.role) < credit_priority(&current.role));
        if leads {
            *credited = Some(Contributor {
                person_id: row.person_id,
                name: row.name,
                role: row.role,
            });
        }
    }

    let numbers = sqlx::query!(
        r#"SELECT cn.work_id, cn.value
           FROM work_catalog_number cn JOIN work w ON w.id = cn.work_id
           JOIN library l ON l.id = w.library_id
           WHERE (?1 IS NULL OR w.library_id = ?1) AND (NOT ?2 OR l.private = 0)
             AND (?3 IS NULL OR w.id IN (SELECT work_id FROM work_contributor WHERE person_id = ?3))
           ORDER BY cn.id"#,
        library,
        public_only,
        credited_to
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in numbers {
        works[index[&row.work_id]].summary.numbers.push(row.value);
    }
    for work in &mut works {
        // A stable sort, so numbers sharing a scheme keep the order they were entered in
        work.summary.numbers.sort_by_key(|number| {
            CatalogNumber::parse(number)
                .scheme_priority()
                .unwrap_or(usize::MAX)
        });
    }
    Ok(works)
}

pub(crate) async fn persons(
    conn: &mut SqliteConnection,
    library: Option<i64>,
    public_only: bool,
) -> Result<Vec<InLibrary<PersonSummary>>> {
    let persons = sqlx::query!(
        r#"SELECT per.id AS "id!", per.name, l.id AS "library_id!", l.name AS library_name,
                  (SELECT COUNT(DISTINCT work_id) FROM work_contributor WHERE person_id = per.id)
                + (SELECT COUNT(*) FROM publication_contributor pc
                   WHERE pc.person_id = per.id
                     AND NOT EXISTS (SELECT 1 FROM publication_work pw
                                     WHERE pw.publication_id = pc.publication_id))
                  AS "works!: i64"
           FROM person per JOIN library l ON l.id = per.library_id
           WHERE (?1 IS NULL OR per.library_id = ?1) AND (NOT ?2 OR l.private = 0)
           ORDER BY l.name, per.sort_name"#,
        library,
        public_only
    )
    .fetch_all(conn)
    .await?;
    Ok(persons
        .into_iter()
        .map(|row| InLibrary {
            library_id: row.library_id,
            library_name: row.library_name,
            summary: PersonSummary {
                id: row.id,
                name: row.name,
                works: row.works,
            },
        })
        .collect())
}

fn index_of<T>(loaded: &[InLibrary<T>], id: fn(&T) -> i64) -> HashMap<i64, usize> {
    loaded
        .iter()
        .enumerate()
        .map(|(i, entity)| (id(&entity.summary), i))
        .collect()
}
