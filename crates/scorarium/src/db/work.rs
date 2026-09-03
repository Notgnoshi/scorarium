use std::collections::HashMap;

use sqlx::SqlitePool;

use crate::db::person::Contributor;

#[derive(Debug, PartialEq, Eq)]
pub struct Work {
    pub id: i64,
    pub library_id: i64,
    pub title: String,
    pub key: Option<String>,
    pub time_signature: Option<String>,
    pub instrumentation: Option<String>,
    pub catalog_numbers: Vec<String>,
    pub contributors: Vec<Contributor>,
}

impl Work {
    /// The contributors credited with `role`, for listings with one column per role.
    pub fn with_role(&self, role: &str) -> Vec<&Contributor> {
        self.contributors
            .iter()
            .filter(|c| c.role == role)
            .collect()
    }

    /// The roles one person is credited with, for pages about that person.
    pub fn roles_of(&self, person_id: i64) -> Vec<&str> {
        self.contributors
            .iter()
            .filter(|c| c.person_id == person_id)
            .map(|c| c.role.as_str())
            .collect()
    }
}

pub struct NewWork<'a> {
    pub library_id: i64,
    pub title: &'a str,
    pub key: Option<&'a str>,
    pub time_signature: Option<&'a str>,
    pub instrumentation: Option<&'a str>,
}

/// One work, or None when it does not exist or belongs to another library.
pub async fn get(pool: &SqlitePool, library_id: i64, id: i64) -> sqlx::Result<Option<Work>> {
    Ok(load(pool, library_id, Some(id), None).await?.pop())
}

/// The works a publication contains, with their children, in arbitrary order.
pub async fn list_in_publication(
    pool: &SqlitePool,
    library_id: i64,
    publication_id: i64,
) -> sqlx::Result<Vec<Work>> {
    load(pool, library_id, None, Some(publication_id)).await
}

/// Load a library's works with their children: the one with `id`, or those contained in
/// `publication_id`.
async fn load(
    pool: &SqlitePool,
    library_id: i64,
    id: Option<i64>,
    publication_id: Option<i64>,
) -> sqlx::Result<Vec<Work>> {
    let mut tx = pool.begin().await?;
    let mut works: Vec<Work> = sqlx::query!(
        "SELECT id, library_id, title, \"key\", time_signature, instrumentation FROM work
         WHERE library_id = ?1
           AND (?2 IS NULL OR id = ?2)
           AND (?3 IS NULL OR id IN (SELECT work_id FROM publication_work WHERE publication_id = ?3))",
        library_id,
        id,
        publication_id
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|row| Work {
        id: row.id,
        library_id: row.library_id,
        title: row.title,
        key: row.key,
        time_signature: row.time_signature,
        instrumentation: row.instrumentation,
        catalog_numbers: Vec::new(),
        contributors: Vec::new(),
    })
    .collect();
    let index: HashMap<i64, usize> = works.iter().enumerate().map(|(i, w)| (w.id, i)).collect();

    let catalog_numbers = sqlx::query!(
        "SELECT work_id, value FROM work_catalog_number
         WHERE work_id IN
            (SELECT id FROM work
             WHERE library_id = ?1
               AND (?2 IS NULL OR id = ?2)
               AND (?3 IS NULL OR id IN (SELECT work_id FROM publication_work WHERE publication_id = ?3)))",
        library_id,
        id,
        publication_id
    )
    .fetch_all(&mut *tx)
    .await?;
    for row in catalog_numbers {
        works[index[&row.work_id]].catalog_numbers.push(row.value);
    }

    let contributors = sqlx::query!(
        "SELECT c.work_id, c.person_id, p.name, c.role
         FROM work_contributor c JOIN person p ON p.id = c.person_id
         WHERE c.work_id IN
            (SELECT id FROM work
             WHERE library_id = ?1
               AND (?2 IS NULL OR id = ?2)
               AND (?3 IS NULL OR id IN (SELECT work_id FROM publication_work WHERE publication_id = ?3)))",
        library_id,
        id,
        publication_id
    )
    .fetch_all(&mut *tx)
    .await?;
    for row in contributors {
        works[index[&row.work_id]].contributors.push(Contributor {
            person_id: row.person_id,
            name: row.name,
            role: row.role,
        });
    }

    tx.commit().await?;
    Ok(works)
}

pub async fn create_work(pool: &SqlitePool, new: &NewWork<'_>) -> sqlx::Result<i64> {
    let result = sqlx::query!(
        "INSERT INTO work (library_id, title, \"key\", time_signature, instrumentation)
         VALUES (?, ?, ?, ?, ?)",
        new.library_id,
        new.title,
        new.key,
        new.time_signature,
        new.instrumentation,
    )
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn create_catalog_number(
    pool: &SqlitePool,
    work_id: i64,
    value: &str,
) -> sqlx::Result<i64> {
    let result = sqlx::query!(
        "INSERT INTO work_catalog_number (work_id, value) VALUES (?, ?)",
        work_id,
        value,
    )
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

/// Record that a publication contains a work. Fails unless both belong to `library_id`.
pub async fn add_to_publication(
    pool: &SqlitePool,
    library_id: i64,
    publication_id: i64,
    work_id: i64,
) -> sqlx::Result<i64> {
    let result = sqlx::query!(
        "INSERT INTO publication_work (library_id, publication_id, work_id) VALUES (?, ?, ?)",
        library_id,
        publication_id,
        work_id,
    )
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

/// Link a person to a work. Fails unless both belong to `library_id`.
pub async fn create_contributor(
    pool: &SqlitePool,
    library_id: i64,
    work_id: i64,
    person_id: i64,
    role: &str,
) -> sqlx::Result<i64> {
    let result = sqlx::query!(
        "INSERT INTO work_contributor (library_id, work_id, person_id, role) VALUES (?, ?, ?, ?)",
        library_id,
        work_id,
        person_id,
        role,
    )
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::publication::{NewPublication, create_publication, list_containing};

    #[sqlx::test]
    async fn list_assembles_children(pool: SqlitePool) {
        let library_id = db::create_library(&pool, "lib").await.unwrap();
        let album = create_publication(
            &pool,
            &NewPublication {
                library_id,
                title: "Album",
                publisher: None,
                year: None,
            },
        )
        .await
        .unwrap();
        let other_album = create_publication(
            &pool,
            &NewPublication {
                library_id,
                title: "Other album",
                publisher: None,
                year: None,
            },
        )
        .await
        .unwrap();
        let mikrokosmos = create_work(
            &pool,
            &NewWork {
                library_id,
                title: "Mikrokosmos",
                key: None,
                time_signature: Some("2/4"),
                instrumentation: Some("piano"),
            },
        )
        .await
        .unwrap();
        let bare = create_work(
            &pool,
            &NewWork {
                library_id,
                title: "Bare",
                key: None,
                time_signature: None,
                instrumentation: None,
            },
        )
        .await
        .unwrap();
        let elsewhere = create_work(
            &pool,
            &NewWork {
                library_id,
                title: "Elsewhere",
                key: None,
                time_signature: None,
                instrumentation: None,
            },
        )
        .await
        .unwrap();
        add_to_publication(&pool, library_id, album, mikrokosmos)
            .await
            .unwrap();
        add_to_publication(&pool, library_id, album, bare)
            .await
            .unwrap();
        add_to_publication(&pool, library_id, other_album, elsewhere)
            .await
            .unwrap();
        add_to_publication(&pool, library_id, other_album, mikrokosmos)
            .await
            .unwrap();
        create_catalog_number(&pool, mikrokosmos, "Sz. 107")
            .await
            .unwrap();
        create_catalog_number(&pool, mikrokosmos, "BB 105")
            .await
            .unwrap();
        let bartok = db::person::create_person(&pool, library_id, "Bela Bartok", "Bartok, Bela")
            .await
            .unwrap();
        create_contributor(&pool, library_id, mikrokosmos, bartok, "composer")
            .await
            .unwrap();

        let mut works = list_in_publication(&pool, library_id, album).await.unwrap();
        works.sort_by_key(|w| w.id);
        works.iter_mut().for_each(|w| w.catalog_numbers.sort());
        assert_eq!(
            works,
            [
                Work {
                    id: mikrokosmos,
                    library_id,
                    title: "Mikrokosmos".into(),
                    key: None,
                    time_signature: Some("2/4".into()),
                    instrumentation: Some("piano".into()),
                    catalog_numbers: vec!["BB 105".into(), "Sz. 107".into()],
                    contributors: vec![Contributor {
                        person_id: bartok,
                        name: "Bela Bartok".into(),
                        role: "composer".into(),
                    }],
                },
                Work {
                    id: bare,
                    library_id,
                    title: "Bare".into(),
                    key: None,
                    time_signature: None,
                    instrumentation: None,
                    catalog_numbers: vec![],
                    contributors: vec![],
                },
            ]
        );

        assert_eq!(
            get(&pool, library_id, mikrokosmos)
                .await
                .unwrap()
                .unwrap()
                .id,
            mikrokosmos
        );
        let other_library = db::create_library(&pool, "other").await.unwrap();
        assert_eq!(get(&pool, other_library, mikrokosmos).await.unwrap(), None);

        let mut containing: Vec<i64> = list_containing(&pool, library_id, mikrokosmos)
            .await
            .unwrap()
            .iter()
            .map(|p| p.id)
            .collect();
        containing.sort();
        assert_eq!(containing, [album, other_album]);
    }

    #[sqlx::test]
    async fn publication_work_must_share_library(pool: SqlitePool) {
        let books = db::create_library(&pool, "books").await.unwrap();
        let scores = db::create_library(&pool, "scores").await.unwrap();
        let publication = create_publication(
            &pool,
            &NewPublication {
                library_id: books,
                title: "Pro Git",
                publisher: None,
                year: None,
            },
        )
        .await
        .unwrap();
        let gymnopedie = create_work(
            &pool,
            &NewWork {
                library_id: scores,
                title: "Gymnopedie No. 1",
                key: None,
                time_signature: None,
                instrumentation: None,
            },
        )
        .await
        .unwrap();

        for library_id in [books, scores] {
            let result = add_to_publication(&pool, library_id, publication, gymnopedie).await;
            assert!(
                matches!(result, Err(sqlx::Error::Database(_))),
                "{result:?}"
            );
        }
    }
}
