use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use scorarium_archive::{CatalogNumber, Entity, NotFound, SuggestField, Suggested, Suggestion};
use serde::{Deserialize, Serialize};

use super::{AppError, OrNotFound, Session, search};
use crate::AppState;

/// What the input has typed so far, plus what the input's neighbours need the archive to know
#[derive(Deserialize)]
pub struct FieldQuery {
    #[serde(default)]
    q: String,
    /// work-number only: the composer input of the same work, resolved by the archive
    composer: Option<String>,
    /// tag only: comma-separated tags already picked
    exclude: Option<String>,
}

#[derive(Serialize)]
pub struct FieldSuggestions {
    /// work-number only: whether the typed text parses as a known scheme, for the indicator
    #[serde(skip_serializing_if = "Option::is_none")]
    recognized: Option<bool>,
    matches: Vec<FieldMatch>,
}

/// One dropdown item.
#[derive(Serialize)]
struct FieldMatch {
    kind: &'static str,
    /// What goes into the input when picked
    value: String,
    exact: bool,
    /// Primary description to display (e.g., title)
    primary: String,
    /// Secondary description to display (e.g., author, composer)
    secondary: String,
    #[serde(flatten)]
    data: Data,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Data {
    Person {
        id: i64,
        name: String,
        works: i64,
    },
    Work {
        id: i64,
        title: String,
        contributor: Option<String>,
        role: Option<String>,
        numbers: Vec<String>,
        /// Whether the number a pick puts in the catalog input uses a known scheme
        #[serde(skip_serializing_if = "Option::is_none")]
        recognized: Option<bool>,
    },
    Publication {
        id: i64,
        title: String,
        people: Vec<String>,
        year: Option<i64>,
    },
    Tag {
        count: i64,
    },
    Plain {},
}

/// How many the dropdown shows: entities, and the small vocabularies narrowed by typing
const ENTITY_LIMIT: usize = 10;
const VOCABULARY_LIMIT: usize = 15;

/// GET /library/{id}/suggest/{kind}
///
/// Requires being logged in
pub async fn field(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path((library_id, kind)): Path<(i64, String)>,
    Query(query): Query<FieldQuery>,
) -> Result<Json<FieldSuggestions>, AppError> {
    let composer = query
        .composer
        .as_deref()
        .map(str::trim)
        .filter(|composer| !composer.is_empty())
        .map(str::to_string);
    let field = match kind.as_str() {
        "person" => SuggestField::Person,
        "work" => SuggestField::Work,
        "work-number" => SuggestField::WorkNumber { composer },
        "publication" => SuggestField::Publication,
        "role" => SuggestField::Role,
        "publisher" => SuggestField::Publisher,
        "tag" => SuggestField::Tag,
        "key" => SuggestField::Key,
        "time-signature" => SuggestField::TimeSignature,
        "instrumentation" => SuggestField::Instrumentation,
        "location" => SuggestField::Location,
        _ => return Err(AppError::from(NotFound)),
    };
    let limit = match field {
        SuggestField::Person
        | SuggestField::Work
        | SuggestField::WorkNumber { .. }
        | SuggestField::Publication => ENTITY_LIMIT,
        _ => VOCABULARY_LIMIT,
    };
    let recognized = match field {
        SuggestField::WorkNumber { .. } => Some(CatalogNumber::parse(&query.q).is_recognized()),
        _ => None,
    };
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let mut suggestions = library.suggest(field, &query.q, false).await?;
    if let Some(exclude) = &query.exclude {
        let chosen: Vec<&str> = exclude.split(',').map(str::trim).collect();
        suggestions.retain(|suggestion| match &suggestion.item {
            Suggested::Tag { name, .. } => !chosen.contains(&name.as_str()),
            _ => true,
        });
    }
    suggestions.truncate(limit);
    Ok(Json(FieldSuggestions {
        recognized,
        matches: suggestions.into_iter().map(shown).collect(),
    }))
}

/// One suggestion as a dropdown item
fn shown(suggestion: Suggestion) -> FieldMatch {
    let exact = suggestion.exact;
    match suggestion.item {
        Suggested::Person(person) => {
            let (kind, primary, secondary) = search::describe(&Entity::Person(person.clone()));
            FieldMatch {
                kind,
                value: person.name.clone(),
                exact,
                primary,
                secondary,
                data: Data::Person {
                    id: person.id,
                    name: person.name,
                    works: person.works,
                },
            }
        }
        Suggested::Publication(publication) => {
            let (kind, primary, secondary) =
                search::describe(&Entity::Publication(publication.clone()));
            FieldMatch {
                kind,
                value: publication.title.clone(),
                exact,
                primary,
                secondary,
                data: Data::Publication {
                    id: publication.id,
                    title: publication.title,
                    people: publication.people,
                    year: publication.year,
                },
            }
        }
        Suggested::Work { work, number } => {
            // A number input puts the matched number in the input and leads with it; a title
            // input puts the title in and leads with that
            let (value, primary, secondary, recognized) = match &number {
                Some(number) => {
                    let (primary, secondary) = search::describe_number(&work, number);
                    (
                        number.clone(),
                        primary,
                        secondary,
                        CatalogNumber::parse(number).is_recognized(),
                    )
                }
                None => {
                    let lead = work.numbers.first();
                    let (_, primary, secondary) = search::describe(&Entity::Work(work.clone()));
                    (
                        work.title.clone(),
                        primary,
                        secondary,
                        lead.is_some_and(|number| CatalogNumber::parse(number).is_recognized()),
                    )
                }
            };
            let has_number = number.is_some() || !work.numbers.is_empty();
            FieldMatch {
                kind: "work",
                value,
                exact,
                primary,
                secondary,
                data: Data::Work {
                    id: work.id,
                    title: work.title,
                    contributor: work.contributor.as_ref().map(|person| person.name.clone()),
                    role: work.contributor.map(|person| person.role),
                    numbers: work.numbers,
                    recognized: has_number.then_some(recognized),
                },
            }
        }
        Suggested::Tag { name, count } => FieldMatch {
            kind: "tag",
            value: name.clone(),
            exact,
            primary: name,
            secondary: format!("{count} tagged"),
            data: Data::Tag { count },
        },
        Suggested::Role(value)
        | Suggested::Publisher(value)
        | Suggested::Key(value)
        | Suggested::TimeSignature(value)
        | Suggested::Instrumentation(value)
        | Suggested::Location(value) => FieldMatch {
            kind: "plain",
            value: value.clone(),
            exact,
            primary: value,
            secondary: String::new(),
            data: Data::Plain {},
        },
    }
}
