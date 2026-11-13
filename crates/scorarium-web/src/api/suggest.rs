use askama::Template;
use axum::extract::{Path, State};
use axum::response::Html;
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::person::PersonId;
use scorarium_engine::suggest::{SuggestField, Suggested};
use scorarium_engine::work::CatalogNumber;

use crate::api::{Query, SUGGESTION_LIMIT};
use crate::auth::ApiSession;
use crate::error::Result;
use crate::page::library::entity::ContainedWork;
use crate::page::library::view;

/// Map a route's field name to the field it suggests values for
///
/// The `person` narrows work suggestions to the works crediting them.
fn field(name: &str, person: Option<PersonId>) -> Option<SuggestField> {
    Some(match name {
        "person" => SuggestField::Person,
        "role" => SuggestField::Role,
        "tag" => SuggestField::Tag,
        "publisher" => SuggestField::Publisher,
        "work" => SuggestField::Work {
            contributor: person,
        },
        "key" => SuggestField::Key,
        "time_signature" => SuggestField::TimeSignature,
        "instrumentation" => SuggestField::Instrumentation,
        "location" => SuggestField::Location,
        _ => return None,
    })
}

struct Entry {
    /// The id a selection records, for entities; `None` for plain values
    suggestion_id: Option<String>,
    text: String,
    draft: bool,
    detail: Option<String>,
}

impl Entry {
    fn value(text: String) -> Entry {
        Entry {
            suggestion_id: None,
            text,
            draft: false,
            detail: None,
        }
    }
}

#[derive(Template)]
#[template(path = "api/suggest.html")]
struct Fragment {
    entries: Vec<Entry>,
    /// When suggesting people or works, the noun and the typed text of a suggestion to create a new one.
    create: Option<(&'static str, String)>,
}

pub async fn get(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((library, name)): Path<(String, String)>,
    axum::extract::Query(query): axum::extract::Query<Query>,
) -> Result<Html<String>> {
    let library = view::load(&engine, &library).await?;
    let person = query.person.as_deref().and_then(|id| id.parse().ok());
    let field = field(&name, person).ok_or(scorarium_engine::Error::NotFound)?;
    let query = query.q.as_deref().unwrap_or_default().trim();
    if query.is_empty() {
        return Ok(Html(String::new()));
    }
    let suggestions = library.suggest(field, query, SUGGESTION_LIMIT).await?;
    let entries = suggestions
        .into_iter()
        .filter_map(|suggestion| match suggestion.item {
            Suggested::Person(person) => Some(Entry {
                suggestion_id: Some(person.id().to_string()),
                text: person.name().to_owned(),
                draft: person.is_draft(),
                detail: None,
            }),
            Suggested::Work { work, .. } => {
                let contained = ContainedWork::from(&work);
                let detail = [
                    contained.catalog_number.as_ref().map(CatalogNumber::as_str),
                    contained
                        .first_contributor()
                        .map(|(contributor, _)| contributor.name.as_str()),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
                Some(Entry {
                    suggestion_id: Some(work.id().to_string()),
                    text: work.title().to_owned(),
                    draft: work.is_draft(),
                    detail: (!detail.is_empty()).then(|| detail.join(", ")),
                })
            }
            Suggested::Publisher(name) => Some(Entry::value(name)),
            Suggested::Role(role) => Some(Entry::value(role.as_str().to_owned())),
            Suggested::Key(text)
            | Suggested::TimeSignature(text)
            | Suggested::Instrumentation(text)
            | Suggested::Location(text) => Some(Entry::value(text)),
            Suggested::Tag { tag, count } => Some(Entry {
                detail: Some(count.to_string()),
                ..Entry::value(tag.to_string())
            }),
            // These belong to fields this route never requests.
            Suggested::Publication(_) => None,
        })
        .collect();
    let noun = match field {
        SuggestField::Person => Some("person"),
        SuggestField::Work { .. } => Some("work"),
        _ => None,
    };
    let fragment = Fragment {
        entries,
        create: noun.map(|noun| (noun, query.to_owned())),
    };
    Ok(Html(fragment.render()?))
}
