use scorarium_archive::fuzzy::{normalize, rank};

use crate::library::{self, LibraryId};
use crate::value::Tag;
use crate::{Engine, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SuggestField {
    Role,
    Publisher,
    Tag,
    Key,
    TimeSignature,
    Instrumentation,
    Location,
}

#[derive(Clone, Debug)]
pub enum Suggested {
    Role(String),
    Publisher(String),
    Tag { tag: Tag, count: usize },
    Key(String),
    TimeSignature(String),
    Instrumentation(String),
    Location(String),
}

#[derive(Clone, Debug)]
pub struct Suggestion {
    /// The search query is equal to the suggestion after normalization
    pub exact: bool,
    pub item: Suggested,
}

const CONVENTIONAL_ROLES: [&str; 5] = ["arranger", "author", "composer", "editor", "translator"];

pub(crate) async fn suggest(
    engine: &Engine,
    library: LibraryId,
    field: SuggestField,
    query: &str,
    limit: usize,
) -> Result<Vec<Suggestion>> {
    let mut conn = engine.begin_read().await?;
    let mut suggestions = match field {
        SuggestField::Role => {
            let stored = sqlx::query_scalar!(
                r#"SELECT DISTINCT role AS "role!" FROM (
                       SELECT role FROM publication_contributor WHERE library_id = ?1
                       UNION
                       SELECT role FROM work_contributor WHERE library_id = ?1
                   )"#,
                library.get()
            )
            .fetch_all(&mut *conn)
            .await?;
            let mut roles: Vec<String> = Vec::new();
            for role in stored {
                if !roles.iter().any(|kept| kept.eq_ignore_ascii_case(&role)) {
                    roles.push(role);
                }
            }
            for role in CONVENTIONAL_ROLES {
                if !roles.iter().any(|kept| kept.eq_ignore_ascii_case(role)) {
                    roles.push(role.to_string());
                }
            }
            roles.sort_by_key(|role| role.to_ascii_lowercase());
            ranked(query, roles, String::as_str, Suggested::Role)
        }
        SuggestField::Publisher => {
            let publishers = sqlx::query_scalar!(
                r#"SELECT DISTINCT publisher AS "value!" FROM publication
                   WHERE library_id = ? AND publisher IS NOT NULL ORDER BY publisher"#,
                library.get()
            )
            .fetch_all(&mut *conn)
            .await?;
            ranked(query, publishers, String::as_str, Suggested::Publisher)
        }
        SuggestField::Tag => {
            let counts = library::tag_counts(&mut conn, library, true).await?;
            ranked(
                query,
                counts,
                |(tag, _): &(Tag, usize)| tag.as_str(),
                |(tag, count)| Suggested::Tag { tag, count },
            )
        }
        SuggestField::Key => {
            let keys = sqlx::query_scalar!(
                r#"SELECT DISTINCT "key" AS "value!" FROM work
                   WHERE library_id = ? AND "key" IS NOT NULL ORDER BY "key""#,
                library.get()
            )
            .fetch_all(&mut *conn)
            .await?;
            ranked(query, keys, String::as_str, Suggested::Key)
        }
        SuggestField::TimeSignature => {
            let time_signatures = sqlx::query_scalar!(
                r#"SELECT DISTINCT time_signature AS "value!" FROM work
                   WHERE library_id = ? AND time_signature IS NOT NULL ORDER BY time_signature"#,
                library.get()
            )
            .fetch_all(&mut *conn)
            .await?;
            ranked(
                query,
                time_signatures,
                String::as_str,
                Suggested::TimeSignature,
            )
        }
        SuggestField::Instrumentation => {
            let instrumentations = sqlx::query_scalar!(
                r#"SELECT DISTINCT instrumentation AS "value!" FROM work
                   WHERE library_id = ? AND instrumentation IS NOT NULL ORDER BY instrumentation"#,
                library.get()
            )
            .fetch_all(&mut *conn)
            .await?;
            ranked(
                query,
                instrumentations,
                String::as_str,
                Suggested::Instrumentation,
            )
        }
        SuggestField::Location => {
            let locations = sqlx::query_scalar!(
                r#"SELECT DISTINCT holding.location AS "value!" FROM holding
                   JOIN publication ON publication.id = holding.publication_id
                   WHERE publication.library_id = ? AND holding.kind = 'physical'
                     AND holding.location IS NOT NULL
                   ORDER BY holding.location"#,
                library.get()
            )
            .fetch_all(&mut *conn)
            .await?;
            ranked(query, locations, String::as_str, Suggested::Location)
        }
    };
    suggestions.truncate(limit);
    Ok(suggestions)
}

/// Rank candidate values against the search query
fn ranked<T>(
    query: &str,
    items: Vec<T>,
    value: fn(&T) -> &str,
    wrap: fn(T) -> Suggested,
) -> Vec<Suggestion> {
    let order: Vec<usize> = if normalize(query).is_empty() {
        (0..items.len()).collect()
    } else {
        let values: Vec<&str> = items.iter().map(value).collect();
        rank(query, &values)
    };
    let mut items: Vec<Option<T>> = items.into_iter().map(Some).collect();
    let mut suggestions: Vec<Suggestion> = order
        .into_iter()
        .map(|i| {
            let item = items[i].take().expect("each index is ranked once");
            Suggestion {
                exact: normalize(value(&item)) == normalize(query),
                item: wrap(item),
            }
        })
        .collect();
    suggestions.sort_by_key(|suggestion| !suggestion.exact);
    suggestions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineOptions;
    use crate::holding::NewHolding;
    use crate::publication::ImportSeed;

    fn values(suggestions: &[Suggestion]) -> Vec<(&str, bool)> {
        suggestions
            .iter()
            .map(|suggestion| {
                let value = match &suggestion.item {
                    Suggested::Role(value) | Suggested::Publisher(value) => value.as_str(),
                    other => panic!("unexpected suggestion {other:?}"),
                };
                (value, suggestion.exact)
            })
            .collect()
    }

    #[tokio::test]
    async fn vocabulary_lists_alphabetically_and_exact_first() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path(), EngineOptions::default())
            .await
            .unwrap();
        let library = engine.create_library("Sheet music", false).await.unwrap();
        for (title, publisher) in [
            ("Russian piano album", "Schirmer"),
            ("Rachmaninoff masterpieces", "Dover"),
            ("Pro Git", "Apress"),
        ] {
            let mut publication = library
                .create_publication(
                    ImportSeed::Title(title.into()),
                    NewHolding::Physical { location: None },
                )
                .await
                .unwrap();
            publication.set_publisher(Some(publisher)).await.unwrap();
        }
        let mut album = library.publications().await.unwrap().remove(0);
        album
            .create_contributor("Sergei Rachmaninoff", "Composer")
            .await
            .unwrap();

        let publishers = library
            .suggest(SuggestField::Publisher, "", 10)
            .await
            .unwrap();
        assert_eq!(
            values(&publishers),
            [("Apress", false), ("Dover", false), ("Schirmer", false)]
        );
        let publishers = library
            .suggest(SuggestField::Publisher, "dover", 10)
            .await
            .unwrap();
        assert_eq!(values(&publishers)[0], ("Dover", true));
        let publishers = library
            .suggest(SuggestField::Publisher, "", 2)
            .await
            .unwrap();
        assert_eq!(publishers.len(), 2);

        // The stored spelling replaces the seeded "composer" and sorts among the seeds
        let roles = library.suggest(SuggestField::Role, "", 10).await.unwrap();
        assert_eq!(
            values(&roles),
            [
                ("arranger", false),
                ("author", false),
                ("Composer", false),
                ("editor", false),
                ("translator", false),
            ]
        );
    }
}
