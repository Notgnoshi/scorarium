use sqlx::SqliteConnection;

use crate::input::ValidationError;

/// Split a tag field on whitespace and check each tag.
pub(crate) fn parse_tags(field: &str) -> Result<Vec<String>, ValidationError> {
    let mut tags: Vec<String> = Vec::new();
    for typed in field.split_whitespace() {
        let tag = typed.to_ascii_lowercase();
        if !tag
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            // Named as it was typed, so the message points at what is on the screen
            return Err(ValidationError::InvalidTag(typed.to_string()));
        }
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    Ok(tags)
}

/// Every distinct tag in the library, alphabetically, for the tag field's suggestions.
pub(crate) async fn list_vocabulary(
    conn: &mut SqliteConnection,
    library_id: i64,
) -> crate::Result<Vec<String>> {
    let tags = sqlx::query_scalar!(
        "SELECT DISTINCT tag FROM tag WHERE library_id = ? ORDER BY tag",
        library_id
    )
    .fetch_all(conn)
    .await?;
    Ok(tags)
}

/// Every tag on a library's publications, as (publication_id, tag), alphabetically.
pub(crate) async fn publication_tags(
    conn: &mut SqliteConnection,
    library_id: i64,
) -> crate::Result<Vec<(i64, String)>> {
    let rows = sqlx::query!(
        r#"SELECT publication_id AS "publication_id!", tag FROM tag
           WHERE library_id = ? AND publication_id IS NOT NULL
           ORDER BY tag"#,
        library_id
    )
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.publication_id, row.tag))
        .collect())
}

/// Every tag on a library's works, as (work_id, tag), alphabetically
pub(crate) async fn work_tags(
    conn: &mut SqliteConnection,
    library_id: i64,
) -> crate::Result<Vec<(i64, String)>> {
    let rows = sqlx::query!(
        r#"SELECT work_id AS "work_id!", tag FROM tag
           WHERE library_id = ? AND work_id IS NOT NULL
           ORDER BY tag"#,
        library_id
    )
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(|row| (row.work_id, row.tag)).collect())
}

/// Rebuild a publication's tags
pub(crate) async fn write_publication_tags(
    conn: &mut SqliteConnection,
    library_id: i64,
    publication_id: i64,
    tags: &[String],
) -> crate::Result<()> {
    sqlx::query!("DELETE FROM tag WHERE publication_id = ?", publication_id)
        .execute(&mut *conn)
        .await?;
    insert_tags(conn, library_id, Some(publication_id), None, tags).await
}

/// Rebuild a work's tags
pub(crate) async fn write_work_tags(
    conn: &mut SqliteConnection,
    library_id: i64,
    work_id: i64,
    tags: &[String],
) -> crate::Result<()> {
    sqlx::query!("DELETE FROM tag WHERE work_id = ?", work_id)
        .execute(&mut *conn)
        .await?;
    insert_tags(conn, library_id, None, Some(work_id), tags).await
}

/// Exactly one of `publication_id` and `work_id` is set, which the table's CHECK enforces.
async fn insert_tags(
    conn: &mut SqliteConnection,
    library_id: i64,
    publication_id: Option<i64>,
    work_id: Option<i64>,
    tags: &[String],
) -> crate::Result<()> {
    for tag in tags {
        sqlx::query!(
            "INSERT INTO tag (library_id, publication_id, work_id, tag) VALUES (?, ?, ?, ?)",
            library_id,
            publication_id,
            work_id,
            tag
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(field: &str) -> Vec<String> {
        parse_tags(field).unwrap()
    }

    #[test]
    fn a_field_is_a_set_of_lowercase_slugs() {
        assert_eq!(
            parsed("Christmas PIANO duet"),
            ["christmas", "piano", "duet"]
        );
        assert_eq!(parsed("christmas christmas"), ["christmas"]);
        assert_eq!(parsed("  \n "), [] as [String; 0]);
        assert_eq!(parsed("want-to_learn2"), ["want-to_learn2"]);
    }

    #[test]
    fn a_tag_that_is_not_a_slug_names_itself() {
        // Nothing is transformed but case, so a tag that is not a slug is refused rather than
        // quietly rewritten into one
        assert_eq!(
            parse_tags("christmas christmas!"),
            Err(ValidationError::InvalidTag("christmas!".into()))
        );
        assert_eq!(
            parse_tags("Noël"),
            Err(ValidationError::InvalidTag("Noël".into()))
        );
    }
}
