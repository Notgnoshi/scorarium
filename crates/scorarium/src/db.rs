use sqlx::SqlitePool;

/// A named container of publications.
#[derive(Debug)]
pub struct Library {
    pub id: i64,
    pub name: String,
}

/// Create a new library, returning its id.
pub async fn create_library(pool: &SqlitePool, name: &str) -> sqlx::Result<i64> {
    let result = sqlx::query!("INSERT INTO library (name) VALUES (?)", name)
        .execute(pool)
        .await?;
    Ok(result.last_insert_rowid())
}

/// List all libraries, sorted by name.
pub async fn list_libraries(pool: &SqlitePool) -> sqlx::Result<Vec<Library>> {
    sqlx::query_as!(Library, "SELECT id, name FROM library ORDER BY name")
        .fetch_all(pool)
        .await
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;

    use super::*;

    #[sqlx::test]
    async fn create_and_list_libraries(pool: SqlitePool) {
        create_library(&pool, "lib2").await.unwrap();
        create_library(&pool, "lib1").await.unwrap();

        let libraries = list_libraries(&pool).await.unwrap();
        let names: Vec<&str> = libraries.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["lib1", "lib2"]);
    }
}
