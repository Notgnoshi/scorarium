use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use scorarium_archive::identifier::{self, Kind};
use scorarium_archive::{CatalogNumber, Entity, NotFound, SuggestField, Suggested, Suggestion};
use scorarium_client::open_library::WorkHit;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use super::{AppError, OrNotFound, Session, search};
use crate::AppState;
use crate::enrich::open_library;

/// What the input has typed so far, plus what the input's neighbours need the archive to know
#[derive(Deserialize)]
pub struct FieldQuery {
    #[serde(default)]
    q: String,
    /// work-number only: the composer input of the same work, resolved by the archive
    composer: Option<String>,
    /// tag only: comma-separated tags already picked
    exclude: Option<String>,
    #[serde(default)]
    source: Source,
}

#[derive(Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Source {
    #[default]
    Local,
    External,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    href: Option<String>,
    #[serde(flatten)]
    data: Data,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Reference {
    Local {
        id: i64,
    },
    External {
        /// The source's display name
        source: String,
        /// The record's id within that source, such as an Open Library work OLID
        source_id: String,
    },
}

#[derive(Serialize)]
#[serde(untagged)]
enum Data {
    Person {
        reference: Reference,
        name: String,
    },
    Work {
        reference: Reference,
        title: String,
        contributor: Option<String>,
        role: Option<String>,
        contributor_id: Option<i64>,
        numbers: Vec<String>,
        /// Whether the number a pick puts in the catalog input uses a known scheme
        #[serde(skip_serializing_if = "Option::is_none")]
        recognized: Option<bool>,
    },
    Publication {
        reference: Reference,
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
const EXTERNAL_DEADLINE: Duration = Duration::from_secs(3);

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
    if query.source == Source::External && field != SuggestField::Publication {
        return Err(AppError::from(NotFound));
    }
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
    if query.source == Source::External {
        return Ok(Json(FieldSuggestions {
            recognized: None,
            matches: external(&state, &query.q).await,
        }));
    }
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

async fn external(state: &AppState, q: &str) -> Vec<FieldMatch> {
    let q = q.trim();
    if !worth_searching(q) {
        return Vec::new();
    }
    let deadline = Instant::now() + EXTERNAL_DEADLINE;
    open_library::search_titles(&state.sources.open_library(), q, deadline)
        .await
        .into_iter()
        .map(candidate)
        .collect()
}

fn worth_searching(q: &str) -> bool {
    if identifier::looks_like_identifier(q) {
        return false;
    }
    let mut words = q.split_whitespace();
    match (words.next(), words.next()) {
        (Some(_), Some(_)) => true,
        (Some(word), None) => word.chars().count() >= 6,
        (None, _) => false,
    }
}

fn candidate(hit: WorkHit) -> FieldMatch {
    let isbn = hit.edition.as_ref().and_then(|edition| {
        edition
            .isbn_13
            .iter()
            .chain(&edition.isbn_10)
            .find_map(|raw| identifier::normalize(Kind::Isbn, raw).ok())
            .map(|normalized| normalized.as_str().to_string())
    });
    let title = match &hit.subtitle {
        Some(subtitle) => format!("{}: {subtitle}", hit.title),
        None => hit.title.clone(),
    };
    let authors: Vec<String> = hit.authors.into_iter().map(|author| author.name).collect();
    let secondary = authors.join(", ");
    FieldMatch {
        kind: "publication",
        // An ISBN goes through the entry page's lookup on submit, so the review page opens seeded
        value: isbn.unwrap_or_else(|| title.clone()),
        exact: false,
        primary: title.clone(),
        secondary,
        href: None,
        data: Data::Publication {
            reference: Reference::External {
                source: "Open Library".to_string(),
                source_id: hit.olid,
            },
            title,
            people: authors,
            year: hit.first_publish_year.map(i64::from),
        },
    }
}

fn item(entity: Entity, exact: bool) -> FieldMatch {
    let (kind, primary, secondary) = search::describe(&entity);
    let (value, data) = match entity {
        Entity::Person(person) => (
            person.name.clone(),
            Data::Person {
                reference: Reference::Local { id: person.id },
                name: person.name,
            },
        ),
        Entity::Publication(publication) => (
            publication.title.clone(),
            Data::Publication {
                reference: Reference::Local { id: publication.id },
                title: publication.title,
                people: publication.people,
                year: publication.year,
            },
        ),
        Entity::Work(work) => (
            work.title.clone(),
            Data::Work {
                reference: Reference::Local { id: work.id },
                title: work.title,
                contributor: work.contributor.as_ref().map(|person| person.name.clone()),
                contributor_id: work.contributor.as_ref().map(|person| person.person_id),
                role: work.contributor.map(|person| person.role),
                // A title pick puts the work's first number in the catalog input
                recognized: work
                    .numbers
                    .first()
                    .map(|number| CatalogNumber::parse(number).is_recognized()),
                numbers: work.numbers,
            },
        ),
    };
    FieldMatch {
        kind,
        value,
        exact,
        primary,
        secondary,
        href: None,
        data,
    }
}

/// One suggestion as a dropdown item
fn shown(suggestion: Suggestion) -> FieldMatch {
    let exact = suggestion.exact;
    match suggestion.item {
        Suggested::Person(person) => item(Entity::Person(person), exact),
        Suggested::Publication(publication) => item(Entity::Publication(publication), exact),
        Suggested::Work { work, number: None } => item(Entity::Work(work), exact),
        // A number input puts the matched number in the input and leads with it, rather than the
        // title a work input would lead with
        Suggested::Work {
            work,
            number: Some(number),
        } => {
            let (primary, secondary) = search::describe_number(&work, &number);
            let recognized = CatalogNumber::parse(&number).is_recognized();
            let mut shown = item(Entity::Work(work), exact);
            shown.value = number;
            shown.primary = primary;
            shown.secondary = secondary;
            if let Data::Work {
                recognized: slot, ..
            } = &mut shown.data
            {
                *slot = Some(recognized);
            }
            shown
        }
        Suggested::Tag { name, count } => FieldMatch {
            kind: "tag",
            value: name.clone(),
            exact,
            primary: name,
            secondary: format!("{count} tagged"),
            href: None,
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
            href: None,
            data: Data::Plain {},
        },
    }
}

#[derive(Deserialize)]
pub struct TitleQuery {
    #[serde(default)]
    q: String,
}

/// GET /suggest/title
pub async fn title(
    session: Option<Session>,
    State(state): State<Arc<AppState>>,
    Query(query): Query<TitleQuery>,
) -> Result<Json<FieldSuggestions>, AppError> {
    let hits = state
        .archive
        .suggest_titles(&query.q, session.is_none())
        .await?;
    let matches = hits
        .into_iter()
        .take(ENTITY_LIMIT)
        .map(|hit| {
            let href = search::href(hit.library_id, &hit.entity);
            // Results span every library, so each says which one it is in
            let mut shown = item(hit.entity, false);
            shown.secondary = match shown.secondary.as_str() {
                "" => format!("in {}", hit.library_name),
                secondary => format!("{secondary} in {}", hit.library_name),
            };
            shown.href = Some(href);
            shown
        })
        .collect();
    Ok(Json(FieldSuggestions {
        recognized: None,
        matches,
    }))
}
