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
