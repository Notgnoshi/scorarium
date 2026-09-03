use sqlx::SqlitePool;

#[derive(Debug, PartialEq, Eq)]
pub struct Person {
    pub id: i64,
    pub library_id: i64,
    pub name: String,
    pub sort_name: String,
}

/// A person credited on a publication or work, as read back.
#[derive(Debug, PartialEq, Eq)]
pub struct Contributor {
    pub person_id: i64,
    pub name: String,
    pub role: String,
}

/// One person, or None when they do not exist or belong to another library.
pub async fn get(pool: &SqlitePool, library_id: i64, id: i64) -> sqlx::Result<Option<Person>> {
    sqlx::query_as!(
        Person,
        "SELECT id, library_id, name, sort_name FROM person WHERE library_id = ? AND id = ?",
        library_id,
        id
    )
    .fetch_optional(pool)
    .await
}

/// Persons credited with `role` on any publication or work in the library, sorted by sort name.
pub async fn list_with_role(
    pool: &SqlitePool,
    library_id: i64,
    role: &str,
) -> sqlx::Result<Vec<Person>> {
    sqlx::query_as!(
        Person,
        "SELECT id, library_id, name, sort_name FROM person
         WHERE library_id = ?1
           AND (id IN (SELECT person_id FROM publication_contributor WHERE role = ?2)
                OR id IN (SELECT person_id FROM work_contributor WHERE role = ?2))
         ORDER BY sort_name",
        library_id,
        role
    )
    .fetch_all(pool)
    .await
}

/// The distinct contributor roles used anywhere in the library, sorted.
pub async fn list_roles(pool: &SqlitePool, library_id: i64) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar!(
        "SELECT role FROM publication_contributor WHERE library_id = ?1
         UNION
         SELECT role FROM work_contributor WHERE library_id = ?1
         ORDER BY role",
        library_id
    )
    .fetch_all(pool)
    .await
}

pub async fn create_person(
    pool: &SqlitePool,
    library_id: i64,
    name: &str,
    sort_name: &str,
) -> sqlx::Result<i64> {
    let result = sqlx::query!(
        "INSERT INTO person (library_id, name, sort_name) VALUES (?, ?, ?)",
        library_id,
        name,
        sort_name,
    )
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

/// Link a person to a publication. Fails unless both belong to `library_id`.
pub async fn create_contributor(
    pool: &SqlitePool,
    library_id: i64,
    publication_id: i64,
    person_id: i64,
    role: &str,
) -> sqlx::Result<i64> {
    let result = sqlx::query!(
        "INSERT INTO publication_contributor (library_id, publication_id, person_id, role)
         VALUES (?, ?, ?, ?)",
        library_id,
        publication_id,
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
    use crate::db::publication::{NewPublication, create_publication};

    #[sqlx::test]
    async fn get_is_scoped_to_library(pool: SqlitePool) {
        let library_id = db::create_library(&pool, "lib").await.unwrap();
        let other_library = db::create_library(&pool, "other").await.unwrap();
        let id = create_person(&pool, library_id, "Erik Satie", "Satie, Erik")
            .await
            .unwrap();

        assert_eq!(
            get(&pool, library_id, id).await.unwrap(),
            Some(Person {
                id,
                library_id,
                name: "Erik Satie".into(),
                sort_name: "Satie, Erik".into(),
            })
        );
        assert_eq!(get(&pool, other_library, id).await.unwrap(), None);
        assert_eq!(get(&pool, library_id, id + 1).await.unwrap(), None);
    }

    #[sqlx::test]
    async fn list_with_role_spans_publications_and_works(pool: SqlitePool) {
        let library_id = db::create_library(&pool, "lib").await.unwrap();
        let second = db::create_library(&pool, "second").await.unwrap();
        let publication = create_publication(
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
        let elsewhere = create_publication(
            &pool,
            &NewPublication {
                library_id: second,
                title: "Elsewhere",
                publisher: None,
                year: None,
            },
        )
        .await
        .unwrap();

        // Sort names chosen so that insertion order differs from sort order
        let work_only = create_person(&pool, library_id, "Work Only", "Only, Work")
            .await
            .unwrap();
        db::work::create_contributor(&pool, library_id, work, work_only, "composer")
            .await
            .unwrap();
        let publication_only =
            create_person(&pool, library_id, "Publication Only", "Only, Publication")
                .await
                .unwrap();
        create_contributor(&pool, library_id, publication, publication_only, "composer")
            .await
            .unwrap();
        let both = create_person(&pool, library_id, "Both", "Both")
            .await
            .unwrap();
        create_contributor(&pool, library_id, publication, both, "composer")
            .await
            .unwrap();
        db::work::create_contributor(&pool, library_id, work, both, "composer")
            .await
            .unwrap();
        let editor = create_person(&pool, library_id, "Editor", "Editor")
            .await
            .unwrap();
        create_contributor(&pool, library_id, publication, editor, "editor")
            .await
            .unwrap();
        let far = create_person(&pool, second, "Elsewhere", "Elsewhere")
            .await
            .unwrap();
        create_contributor(&pool, second, elsewhere, far, "composer")
            .await
            .unwrap();

        let composers = list_with_role(&pool, library_id, "composer").await.unwrap();
        assert_eq!(
            composers,
            [
                Person {
                    id: both,
                    library_id,
                    name: "Both".into(),
                    sort_name: "Both".into(),
                },
                Person {
                    id: publication_only,
                    library_id,
                    name: "Publication Only".into(),
                    sort_name: "Only, Publication".into(),
                },
                Person {
                    id: work_only,
                    library_id,
                    name: "Work Only".into(),
                    sort_name: "Only, Work".into(),
                },
            ]
        );
        assert_eq!(
            list_roles(&pool, library_id).await.unwrap(),
            ["composer", "editor"]
        );
        assert_eq!(list_roles(&pool, second).await.unwrap(), ["composer"]);
    }

    #[sqlx::test]
    async fn contributor_must_share_library(pool: SqlitePool) {
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
        let satie = create_person(&pool, scores, "Erik Satie", "Satie, Erik")
            .await
            .unwrap();

        for library_id in [books, scores] {
            let result =
                create_contributor(&pool, library_id, publication, satie, "composer").await;
            assert!(
                matches!(result, Err(sqlx::Error::Database(_))),
                "{result:?}"
            );
        }
    }
}
