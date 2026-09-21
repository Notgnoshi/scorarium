use std::collections::HashMap;

use sqlx::SqliteConnection;

use crate::Result;
use crate::catalog::CatalogNumber;
use crate::fuzzy::{normalize, rank};
use crate::summary::{self, PersonSummary, PublicationSummary, WorkSummary};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entity {
    Publication(PublicationSummary),
    Work(WorkSummary),
    Person(PersonSummary),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub library_id: i64,
    pub library_name: String,
    pub entity: Entity,
}

/// Publications, works, and people the typed words name, best first.
pub(crate) async fn search(
    conn: &mut SqliteConnection,
    typed: &str,
    public_only: bool,
) -> Result<Vec<SearchHit>> {
    if normalize(typed).is_empty() {
        return Ok(Vec::new());
    }
    let publication_text = texts_of_publications(conn, public_only).await?;
    let work_text = texts_of_works(conn, public_only).await?;
    let mut candidates: Vec<(SearchHit, String)> = Vec::new();
    for found in summary::publications(conn, None, public_only).await? {
        let text = publication_text
            .get(&found.summary.id)
            .cloned()
            .unwrap_or_default();
        candidates.push((
            hit(
                found.library_id,
                found.library_name,
                Entity::Publication(found.summary),
            ),
            text,
        ));
    }
    for found in summary::works(conn, None, public_only, None).await? {
        let mut text = work_text
            .get(&found.summary.id)
            .cloned()
            .unwrap_or_default();
        // Each number is carried twice: as written, and as one word, so it is found whether it was
        // typed "Op. 9 No. 2" or "op9no2"
        for number in &found.summary.numbers {
            text.push(' ');
            text.push_str(&CatalogNumber::parse(number).squashed());
        }
        candidates.push((
            hit(
                found.library_id,
                found.library_name,
                Entity::Work(found.summary),
            ),
            text,
        ));
    }
    for found in summary::persons(conn, None, public_only, None).await? {
        let text = found.summary.name.clone();
        candidates.push((
            hit(
                found.library_id,
                found.library_name,
                Entity::Person(found.summary),
            ),
            text,
        ));
    }
    Ok(ranked(typed, candidates))
}

/// Publications and works whose title matches, for the navbar typeahead
pub(crate) async fn suggest_titles(
    conn: &mut SqliteConnection,
    typed: &str,
    public_only: bool,
) -> Result<Vec<SearchHit>> {
    if normalize(typed).is_empty() {
        return Ok(Vec::new());
    }
    let mut candidates: Vec<(SearchHit, String)> = Vec::new();
    for found in summary::publications(conn, None, public_only).await? {
        let title = found.summary.title.clone();
        candidates.push((
            hit(
                found.library_id,
                found.library_name,
                Entity::Publication(found.summary),
            ),
            title,
        ));
    }
    for found in summary::works(conn, None, public_only, None).await? {
        let title = found.summary.title.clone();
        candidates.push((
            hit(
                found.library_id,
                found.library_name,
                Entity::Work(found.summary),
            ),
            title,
        ));
    }
    Ok(ranked(typed, candidates))
}

fn hit(library_id: i64, library_name: String, entity: Entity) -> SearchHit {
    SearchHit {
        library_id,
        library_name,
        entity,
    }
}

/// Rank the candidates by their text and keep the hits.
fn ranked(typed: &str, candidates: Vec<(SearchHit, String)>) -> Vec<SearchHit> {
    let texts: Vec<&str> = candidates.iter().map(|(_, text)| text.as_str()).collect();
    let order = rank(typed, &texts);
    let mut candidates: Vec<Option<SearchHit>> =
        candidates.into_iter().map(|(hit, _)| Some(hit)).collect();
    order
        .into_iter()
        .map(|i| candidates[i].take().expect("each index is ranked once"))
        .collect()
}

/// What each publication is found by, by publication id
async fn texts_of_publications(
    conn: &mut SqliteConnection,
    public_only: bool,
) -> Result<HashMap<i64, String>> {
    let rows = sqlx::query!(
        r#"SELECT p.id AS "id!",
                  p.title || ' ' || coalesce((SELECT group_concat(per.name, ' ')
                                              FROM publication_contributor c
                                              JOIN person per ON per.id = c.person_id
                                              WHERE c.publication_id = p.id), '')
                  AS "text!: String"
           FROM publication p JOIN library l ON l.id = p.library_id
           WHERE NOT ?1 OR l.private = 0"#,
        public_only
    )
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(|row| (row.id, row.text)).collect())
}

/// What each work is found by, by work id
async fn texts_of_works(
    conn: &mut SqliteConnection,
    public_only: bool,
) -> Result<HashMap<i64, String>> {
    let rows = sqlx::query!(
        r#"SELECT w.id AS "id!",
                  w.title || ' ' || coalesce((SELECT group_concat(per.name, ' ')
                                              FROM work_contributor c
                                              JOIN person per ON per.id = c.person_id
                                              WHERE c.work_id = w.id), '')
                  || ' ' || coalesce((SELECT group_concat(value, ' ') FROM work_catalog_number
                                      WHERE work_id = w.id), '')
                  AS "text!: String"
           FROM work w JOIN library l ON l.id = w.library_id
           WHERE NOT ?1 OR l.private = 0"#,
        public_only
    )
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(|row| (row.id, row.text)).collect())
}
