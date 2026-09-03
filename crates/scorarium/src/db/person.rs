use sqlx::SqlitePool;

/// A person credited on a publication or work, as read back.
#[derive(Debug, PartialEq, Eq)]
pub struct Contributor {
    pub person_id: i64,
    pub name: String,
    pub role: String,
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
