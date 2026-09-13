use std::collections::BTreeSet;

use sqlx::SqliteConnection;

use crate::catalog::CatalogNumber;
use crate::fuzzy::{normalize, rank};
use crate::summary::{self, PersonSummary, PublicationSummary, WorkSummary};
use crate::tag::TagCount;
use crate::{Result, person, tag};

/// Which field to generate suggestions for
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SuggestField {
    Person,
    Work,
    WorkNumber { composer: Option<String> },
    Publication,
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
    Person(PersonSummary),
    /// `number` is the catalog number that matched; None from a title input
    Work {
        work: WorkSummary,
        number: Option<String>,
    },
    Publication(PublicationSummary),
    Role(String),
    Publisher(String),
    Tag {
        name: String,
        count: i64,
    },
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
    // An entity field has no small vocabulary to fall back on, so nothing typed means nothing to
    // yield as suggestions.
    if normalize(typed).is_empty()
        && matches!(
            field,
            SuggestField::Person
                | SuggestField::Work
                | SuggestField::WorkNumber { .. }
                | SuggestField::Publication
        )
    {
        return Ok(Vec::new());
    }
    Ok(match field {
        SuggestField::Person => {
            let persons = summary::persons(conn, Some(library_id), false)
                .await?
                .into_iter()
                .map(|found| found.summary)
                .collect();
            ranked_entities(
                typed,
                persons,
                |person| person.name.clone(),
                |person| person.name.as_str(),
                Suggested::Person,
            )
        }
        SuggestField::Work => {
            let works = summary::works(conn, Some(library_id), false, None)
                .await?
                .into_iter()
                .map(|found| found.summary)
                .collect();
            ranked_entities(
                typed,
                works,
                // A work is found by what identifies it, not by its title alone
                |work| {
                    let mut text = work.title.clone();
                    let credited = work.contributor.iter().map(|person| &person.name);
                    for part in credited.chain(work.numbers.iter()) {
                        text.push(' ');
                        text.push_str(part);
                    }
                    text
                },
                |work| work.title.as_str(),
                |work| Suggested::Work { work, number: None },
            )
        }
        SuggestField::WorkNumber { composer } => {
            // The composer is matched by an exact name and never fuzzily; guessing wrong would
            // silently offer one composer's numbers while the user reads another's name.
            let credited_to = match &composer {
                Some(name) => summary::persons(conn, Some(library_id), false)
                    .await?
                    .into_iter()
                    .find(|person| is_exact(name, &person.summary.name))
                    .map(|person| person.summary.id),
                None => None,
            };
            let works = summary::works(conn, Some(library_id), false, credited_to)
                .await?
                .into_iter()
                .map(|found| found.summary)
                .collect();
            work_numbers(typed, works)
        }
        SuggestField::Publication => {
            let publications = summary::publications(conn, Some(library_id), false)
                .await?
                .into_iter()
                .map(|found| found.summary)
                .collect();
            ranked_entities(
                typed,
                publications,
                |publication| {
                    let mut text = publication.title.clone();
                    for person in &publication.people {
                        text.push(' ');
                        text.push_str(person);
                    }
                    text
                },
                |publication| publication.title.as_str(),
                Suggested::Publication,
            )
        }
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
    let mut items: Vec<Option<T>> = items.into_iter().map(Some).collect();
    order
        .into_iter()
        .map(|i| {
            let item = items[i].take().expect("each index is ranked once");
            Suggestion {
                exact: is_exact(typed, value(&item)),
                item: wrap(item),
            }
        })
        .collect()
}

/// Rank entities by the text their dropdown item shows, best first.
fn ranked_entities<T>(
    typed: &str,
    entities: Vec<T>,
    text: fn(&T) -> String,
    name: fn(&T) -> &str,
    wrap: fn(T) -> Suggested,
) -> Vec<Suggestion> {
    let texts: Vec<String> = entities.iter().map(text).collect();
    let mut entities: Vec<Option<T>> = entities.into_iter().map(Some).collect();
    rank(typed, &texts)
        .into_iter()
        .map(|i| {
            let entity = entities[i].take().expect("each index is ranked once");
            Suggestion {
                exact: is_exact(typed, name(&entity)),
                item: wrap(entity),
            }
        })
        .collect()
}

fn is_exact(typed: &str, name: &str) -> bool {
    normalize(name) == normalize(typed)
}

/// Every catalog number that fits what was typed, best first.
fn work_numbers(typed: &str, works: Vec<WorkSummary>) -> Vec<Suggestion> {
    let wanted = CatalogNumber::parse(typed);
    let (mut same, mut begun, mut other) = (Vec::new(), Vec::new(), Vec::new());
    for work in &works {
        for number in &work.numbers {
            let parsed = CatalogNumber::parse(number);
            if parsed.matches(&wanted) {
                same.push((work, number, parsed));
            } else if parsed.starts_with(&wanted) {
                begun.push((work, number, parsed));
            } else {
                other.push((work, number));
            }
        }
    }
    // The structural tiers read as a catalog does: by number, then by the work it belongs to
    for tier in [&mut same, &mut begun] {
        tier.sort_by(|(work, _, number), (its_work, _, its_number)| {
            number
                .cmp(its_number)
                .then_with(|| work.title.cmp(&its_work.title))
        });
    }

    let offered = |work: &WorkSummary, number: &String, exact: bool| Suggestion {
        exact,
        item: Suggested::Work {
            work: work.clone(),
            number: Some(number.clone()),
        },
    };
    let mut suggestions: Vec<Suggestion> = same
        .iter()
        .map(|(work, number, _)| offered(work, number, true))
        .chain(
            begun
                .iter()
                .map(|(work, number, _)| offered(work, number, false)),
        )
        .collect();
    // What is left is found by everything that identifies the work, not by the number alone
    let texts: Vec<String> = other
        .iter()
        .map(|(work, number)| {
            let mut text = format!("{number} {}", work.title);
            if let Some(person) = &work.contributor {
                text.push(' ');
                text.push_str(&person.name);
            }
            text
        })
        .collect();
    for i in rank(typed, &texts) {
        let (work, number) = other[i];
        suggestions.push(offered(work, number, false));
    }
    suggestions
}
