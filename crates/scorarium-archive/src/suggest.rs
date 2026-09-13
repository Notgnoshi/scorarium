use std::collections::BTreeSet;

use sqlx::SqliteConnection;

use crate::fuzzy::{normalize, rank};
use crate::tag::TagCount;
use crate::{Result, person, tag};

/// Which field to generate suggestions for
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SuggestField {
    Role,
    Publisher,
    Tag,
    Key,
    TimeSignature,
    Instrumentation,
    Location,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Suggested {
    Role(String),
    Publisher(String),
    Tag { name: String, count: i64 },
    Key(String),
    TimeSignature(String),
    Instrumentation(String),
    Location(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    /// The typed text exactly equals the suggestion after normalization
    pub exact: bool,
    pub item: Suggested,
}

/// Roles every library offers before it has credited anyone
const CONVENTIONAL_ROLES: [&str; 5] = ["arranger", "author", "composer", "editor", "translator"];

pub(crate) async fn suggest(
    conn: &mut SqliteConnection,
    library_id: i64,
    field: SuggestField,
    typed: &str,
    public_suggestions_only: bool,
) -> Result<Vec<Suggestion>> {
    Ok(match field {
        SuggestField::Role => {
            let roles: Vec<String> = person::list_contributor_roles(conn, library_id)
                .await?
                .into_iter()
                .chain(CONVENTIONAL_ROLES.iter().map(|role| role.to_string()))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            ranked(typed, roles, |role: &String| role.as_str(), Suggested::Role)
        }
        SuggestField::Publisher => {
            let publishers = sqlx::query_scalar!(
                r#"SELECT DISTINCT publisher AS "publisher!" FROM publication
                   WHERE library_id = ? AND publisher IS NOT NULL ORDER BY publisher"#,
                library_id
            )
            .fetch_all(conn)
            .await?;
            ranked(
                typed,
                publishers,
                |value: &String| value.as_str(),
                Suggested::Publisher,
            )
        }
        // A tag is shown only to a logged-in viewer, even on a public library's pages
        SuggestField::Tag if public_suggestions_only => Vec::new(),
        SuggestField::Tag => {
            let counts = tag::counts(conn, library_id).await?;
            ranked(
                typed,
                counts,
                |count| &count.tag,
                |count: TagCount| Suggested::Tag {
                    name: count.tag,
                    count: count.uses,
                },
            )
        }
        SuggestField::Key => {
            let keys = sqlx::query_scalar!(
                r#"SELECT DISTINCT "key" AS "key!" FROM work
                   WHERE library_id = ? AND "key" IS NOT NULL ORDER BY "key""#,
                library_id
            )
            .fetch_all(conn)
            .await?;
            ranked(typed, keys, |value: &String| value.as_str(), Suggested::Key)
        }
        SuggestField::TimeSignature => {
            let signatures = sqlx::query_scalar!(
                r#"SELECT DISTINCT time_signature AS "time_signature!" FROM work
                   WHERE library_id = ? AND time_signature IS NOT NULL ORDER BY time_signature"#,
                library_id
            )
            .fetch_all(conn)
            .await?;
            ranked(
                typed,
                signatures,
                |value: &String| value.as_str(),
                Suggested::TimeSignature,
            )
        }
        SuggestField::Instrumentation => {
            let instrumentations = sqlx::query_scalar!(
                r#"SELECT DISTINCT instrumentation AS "instrumentation!" FROM work
                   WHERE library_id = ? AND instrumentation IS NOT NULL ORDER BY instrumentation"#,
                library_id
            )
            .fetch_all(conn)
            .await?;
            ranked(
                typed,
                instrumentations,
                |value: &String| value.as_str(),
                Suggested::Instrumentation,
            )
        }
        // A shelf is shown only to a logged-in viewer, even on a public library's pages
        SuggestField::Location if public_suggestions_only => Vec::new(),
        SuggestField::Location => {
            // The shelf input is on physical copies only, and a file path is no shelf
            let locations = sqlx::query_scalar!(
                r#"SELECT DISTINCT h.location AS "location!" FROM holding h
                   JOIN publication p ON p.id = h.publication_id
                   WHERE p.library_id = ? AND h.kind = 'physical' AND h.location IS NOT NULL
                   ORDER BY h.location"#,
                library_id
            )
            .fetch_all(conn)
            .await?;
            ranked(
                typed,
                locations,
                |value: &String| value.as_str(),
                Suggested::Location,
            )
        }
    })
}

/// Rank items against the typed text, or keep their alphabetical order when nothing is typed.
fn ranked<T>(
    typed: &str,
    items: Vec<T>,
    value: fn(&T) -> &str,
    wrap: fn(T) -> Suggested,
) -> Vec<Suggestion> {
    let order: Vec<usize> = if normalize(typed).is_empty() {
        (0..items.len()).collect()
    } else {
        let values: Vec<&str> = items.iter().map(value).collect();
        rank(typed, &values)
    };
    let typed = normalize(typed);
    let mut items: Vec<Option<T>> = items.into_iter().map(Some).collect();
    order
        .into_iter()
        .map(|i| {
            let item = items[i].take().expect("each index is ranked once");
            Suggestion {
                exact: normalize(value(&item)) == typed,
                item: wrap(item),
            }
        })
        .collect()
}
