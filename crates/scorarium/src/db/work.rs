use sqlx::SqlitePool;

pub struct NewWork<'a> {
    pub library_id: i64,
    pub title: &'a str,
    pub key: Option<&'a str>,
    pub time_signature: Option<&'a str>,
    pub instrumentation: Option<&'a str>,
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
    use crate::db::publication::{NewPublication, create_publication};

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
