use sqlx::SqlitePool;

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

pub async fn create_contributor(
    pool: &SqlitePool,
    publication_id: i64,
    person_id: i64,
    role: &str,
) -> sqlx::Result<i64> {
    let result = sqlx::query!(
        "INSERT INTO publication_contributor (publication_id, person_id, role) VALUES (?, ?, ?)",
        publication_id,
        person_id,
        role,
    )
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}
