use std::collections::HashMap;

use sqlx::{SqliteExecutor, SqlitePool};

use crate::db::person::{self, Contributor};
use crate::publication_form::WorkRow;

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

/// Which contributor a one-line summary of the work shows and edits: its composer, else its
/// author, else the first credited. `contributors` must be in link order.
pub fn lead_contributor(contributors: &[Contributor]) -> Option<usize> {
    contributors
        .iter()
        .position(|c| c.role == "composer")
        .or_else(|| contributors.iter().position(|c| c.role == "author"))
        .or_else(|| (!contributors.is_empty()).then_some(0))
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

/// The works a publication contains, with their children, in the order they were added to it.
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
           AND (?3 IS NULL OR id IN (SELECT work_id FROM publication_work WHERE publication_id = ?3))
         ORDER BY (SELECT id FROM publication_work WHERE work_id = work.id AND publication_id = ?3)",
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
               AND (?3 IS NULL OR id IN (SELECT work_id FROM publication_work WHERE publication_id = ?3)))
         ORDER BY c.id",
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

pub async fn create_work(
    executor: impl SqliteExecutor<'_>,
    new: &NewWork<'_>,
) -> sqlx::Result<i64> {
    let result = sqlx::query!(
        "INSERT INTO work (library_id, title, \"key\", time_signature, instrumentation)
         VALUES (?, ?, ?, ?, ?)",
        new.library_id,
        new.title,
        new.key,
        new.time_signature,
        new.instrumentation,
    )
    .execute(executor)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn create_catalog_number(
    executor: impl SqliteExecutor<'_>,
    work_id: i64,
    value: &str,
) -> sqlx::Result<i64> {
    let result = sqlx::query!(
        "INSERT INTO work_catalog_number (work_id, value) VALUES (?, ?)",
        work_id,
        value,
    )
    .execute(executor)
    .await?;
    Ok(result.last_insert_rowid())
}

/// Record that a publication contains a work. Fails unless both belong to `library_id`.
pub async fn add_to_publication(
    executor: impl SqliteExecutor<'_>,
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
    .execute(executor)
    .await?;
    Ok(result.last_insert_rowid())
}

/// Link a person to a work. Fails unless both belong to `library_id`.
pub async fn create_contributor(
    executor: impl SqliteExecutor<'_>,
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
    .execute(executor)
    .await?;
    Ok(result.last_insert_rowid())
}

/// Apply a form's work rows to a publication.
///
/// A row naming one of the publication's works edits it: its title, and the one contributor the row
/// shows (see [lead_contributor]), leaving the work's other contributors alone. Any other row adds
/// a work. Works no row names are unlinked rather than deleted, leaving
/// `publication::collect_orphans` to decide whether they survive elsewhere.
pub async fn write_contents(
    conn: &mut sqlx::SqliteConnection,
    library_id: i64,
    publication_id: i64,
    rows: &[WorkRow],
) -> sqlx::Result<()> {
    let stored = sqlx::query_scalar!(
        "SELECT work_id FROM publication_work WHERE publication_id = ?",
        publication_id
    )
    .fetch_all(&mut *conn)
    .await?;
    let named: Vec<i64> = rows.iter().filter_map(|row| row.id).collect();
    for work_id in stored.iter().filter(|id| !named.contains(id)) {
        sqlx::query!(
            "DELETE FROM publication_work WHERE publication_id = ? AND work_id = ?",
            publication_id,
            work_id
        )
        .execute(&mut *conn)
        .await?;
    }

    for row in rows {
        let name = &row.contributor.name;
        let role = &row.contributor.role;
        // An id the publication does not contain names nothing this form may edit
        let Some(work_id) = row.id.filter(|id| stored.contains(id)) else {
            let work_id = create_work(
                &mut *conn,
                &NewWork {
                    library_id,
                    title: &row.title,
                    key: None,
                    time_signature: None,
                    instrumentation: None,
                },
            )
            .await?;
            add_to_publication(&mut *conn, library_id, publication_id, work_id).await?;
            if !name.is_empty() {
                let person_id = person::find_or_create(&mut *conn, library_id, name).await?;
                create_contributor(&mut *conn, library_id, work_id, person_id, role).await?;
            }
            continue;
        };

        sqlx::query!(
            "UPDATE work SET title = ? WHERE library_id = ? AND id = ?",
            row.title,
            library_id,
            work_id
        )
        .execute(&mut *conn)
        .await?;

        let mut links = Vec::new();
        let mut contributors = Vec::new();
        for link in sqlx::query!(
            "SELECT c.id, c.person_id, p.name, c.role
             FROM work_contributor c JOIN person p ON p.id = c.person_id
             WHERE c.work_id = ?
             ORDER BY c.id",
            work_id
        )
        .fetch_all(&mut *conn)
        .await?
        {
            links.push(link.id);
            contributors.push(Contributor {
                person_id: link.person_id,
                name: link.name,
                role: link.role,
            });
        }
        let lead = lead_contributor(&contributors);
        let unchanged = match lead {
            Some(i) => contributors[i].name == *name && contributors[i].role == *role,
            None => name.is_empty(),
        };
        if unchanged {
            continue;
        }
        match lead {
            // The lead link is edited rather than replaced, so it keeps its place in link order:
            // that order is what picks the lead for a work with neither a composer nor an author.
            Some(i) if !name.is_empty() => {
                let person_id = person::find_or_create(&mut *conn, library_id, name).await?;
                // Another link the row now duplicates would break UNIQUE (work_id, person_id, role)
                sqlx::query!(
                    "DELETE FROM work_contributor
                     WHERE work_id = ? AND person_id = ? AND role = ? AND id <> ?",
                    work_id,
                    person_id,
                    role,
                    links[i]
                )
                .execute(&mut *conn)
                .await?;
                sqlx::query!(
                    "UPDATE work_contributor SET person_id = ?, role = ? WHERE id = ?",
                    person_id,
                    role,
                    links[i]
                )
                .execute(&mut *conn)
                .await?;
            }
            // The row cleared its contributor, so the work's next credit becomes its lead
            Some(i) => {
                sqlx::query!("DELETE FROM work_contributor WHERE id = ?", links[i])
                    .execute(&mut *conn)
                    .await?;
            }
            None => {
                let person_id = person::find_or_create(&mut *conn, library_id, name).await?;
                create_contributor(&mut *conn, library_id, work_id, person_id, role).await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::publication::{NewPublication, create_publication, list_containing};
    use crate::publication_form::ContributorRow;

    /// Works as (title, [(contributor name, role)]), in the order they came back.
    fn credits(works: &[Work]) -> Vec<(&str, Vec<(&str, &str)>)> {
        works
            .iter()
            .map(|w| {
                let contributors = w
                    .contributors
                    .iter()
                    .map(|c| (c.name.as_str(), c.role.as_str()))
                    .collect();
                (w.title.as_str(), contributors)
            })
            .collect()
    }

    fn contributor(person_id: i64, role: &str) -> Contributor {
        Contributor {
            person_id,
            name: format!("Person {person_id}"),
            role: role.into(),
        }
    }

    #[test]
    fn lead_contributor_prefers_composer_then_author() {
        let composer = [contributor(1, "arranger"), contributor(2, "composer")];
        assert_eq!(lead_contributor(&composer), Some(1));
        let author = [contributor(1, "editor"), contributor(2, "author")];
        assert_eq!(lead_contributor(&author), Some(1));
        let neither = [contributor(1, "editor"), contributor(2, "arranger")];
        assert_eq!(lead_contributor(&neither), Some(0));
        assert_eq!(lead_contributor(&[]), None);
    }

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

    /// A row keeps the work it names, leaves its other contributors alone, and unlinks the works
    /// the form dropped without touching another publication's copy of them.
    #[sqlx::test]
    async fn write_contents_reconciles_works(pool: SqlitePool) {
        let library_id = db::create_library(&pool, "lib").await.unwrap();
        let mut publications = Vec::new();
        for title in ["A", "B"] {
            publications.push(
                create_publication(
                    &pool,
                    &NewPublication {
                        library_id,
                        title,
                        publisher: None,
                        year: None,
                    },
                )
                .await
                .unwrap(),
            );
        }
        let [a, b] = publications[..] else {
            unreachable!()
        };
        let chopin = db::person::create_person(&pool, library_id, "Chopin", "Chopin")
            .await
            .unwrap();
        let liszt = db::person::create_person(&pool, library_id, "Liszt", "Liszt")
            .await
            .unwrap();
        let mut works = Vec::new();
        for (title, publications) in [("Prelude", &[a][..]), ("Etude", &[a, b][..])] {
            let work = create_work(
                &pool,
                &NewWork {
                    library_id,
                    title,
                    key: None,
                    time_signature: None,
                    instrumentation: None,
                },
            )
            .await
            .unwrap();
            for publication in publications {
                add_to_publication(&pool, library_id, *publication, work)
                    .await
                    .unwrap();
            }
            create_contributor(&pool, library_id, work, chopin, "composer")
                .await
                .unwrap();
            works.push(work);
        }
        let [prelude, etude] = works[..] else {
            unreachable!()
        };
        create_contributor(&pool, library_id, prelude, liszt, "arranger")
            .await
            .unwrap();

        let mut tx = pool.begin().await.unwrap();
        write_contents(
            &mut tx,
            library_id,
            a,
            &[
                WorkRow {
                    id: Some(prelude),
                    title: "Prelude in E minor".into(),
                    contributor: ContributorRow {
                        name: "Chopin".into(),
                        role: "composer".into(),
                    },
                },
                WorkRow {
                    id: None,
                    title: "Nocturne".into(),
                    contributor: ContributorRow {
                        name: "Field".into(),
                        role: "composer".into(),
                    },
                },
            ],
        )
        .await
        .unwrap();
        db::publication::collect_orphans(&mut tx, library_id)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let contents = list_in_publication(&pool, library_id, a).await.unwrap();
        assert_eq!(contents[0].id, prelude, "the named work keeps its identity");
        assert_eq!(
            credits(&contents),
            [
                (
                    "Prelude in E minor",
                    // The arranger the row does not show survives, still credited after the lead
                    vec![("Chopin", "composer"), ("Liszt", "arranger")]
                ),
                ("Nocturne", vec![("Field", "composer")]),
            ]
        );
        let nocturne = contents[1].id;
        let elsewhere = list_in_publication(&pool, library_id, b).await.unwrap();
        assert_eq!(
            elsewhere.iter().map(|w| w.id).collect::<Vec<_>>(),
            [etude],
            "another publication's works are not touched"
        );

        // Changing only the role must not hand the row to whoever is credited next
        let mut tx = pool.begin().await.unwrap();
        write_contents(
            &mut tx,
            library_id,
            a,
            &[
                WorkRow {
                    id: Some(prelude),
                    title: "Prelude in E minor".into(),
                    contributor: ContributorRow {
                        name: "Chopin".into(),
                        role: "arranger".into(),
                    },
                },
                WorkRow {
                    id: Some(nocturne),
                    title: "Nocturne".into(),
                    contributor: ContributorRow {
                        name: "Field".into(),
                        role: "composer".into(),
                    },
                },
            ],
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let contents = list_in_publication(&pool, library_id, a).await.unwrap();
        assert_eq!(
            credits(&contents)[0],
            (
                "Prelude in E minor",
                vec![("Chopin", "arranger"), ("Liszt", "arranger")]
            )
        );
        assert_eq!(
            lead_contributor(&contents[0].contributors),
            Some(0),
            "the edited row still shows Chopin, not the arranger it now shares a role with"
        );

        // Changing the row to name a contributor the work already has replaces the lead link
        let mut tx = pool.begin().await.unwrap();
        write_contents(
            &mut tx,
            library_id,
            a,
            &[WorkRow {
                id: Some(prelude),
                title: "Prelude in E minor".into(),
                contributor: ContributorRow {
                    name: "Liszt".into(),
                    role: "arranger".into(),
                },
            }],
        )
        .await
        .unwrap();
        db::publication::collect_orphans(&mut tx, library_id)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let contents = list_in_publication(&pool, library_id, a).await.unwrap();
        assert_eq!(
            credits(&contents),
            [("Prelude in E minor", vec![("Liszt", "arranger")])],
            "Nocturne is unlinked and the lead link is replaced, not duplicated"
        );
        let persons = sqlx::query_scalar!("SELECT name FROM person ORDER BY name")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(
            persons,
            ["Chopin", "Liszt"],
            "Field is collected with Nocturne"
        );
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
