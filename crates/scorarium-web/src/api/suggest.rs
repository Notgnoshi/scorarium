use askama::Template;
use axum::extract::{Path, State};
use axum::response::Html;
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::suggest::{SuggestField, Suggested};

use crate::api::{Query, SUGGESTION_LIMIT};
use crate::auth::ApiSession;
use crate::error::Result;
use crate::page::library::view;

/// Map a route's field name to the field it suggests values for
fn field(name: &str) -> Option<SuggestField> {
    Some(match name {
        "person" => SuggestField::Person,
        "role" => SuggestField::Role,
        "tag" => SuggestField::Tag,
        "key" => SuggestField::Key,
        "time_signature" => SuggestField::TimeSignature,
        "instrumentation" => SuggestField::Instrumentation,
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
    /// When suggesting people, whether there should be a suggestion to create a new person with the
    /// given name.
    create_person: Option<String>,
}

pub async fn get(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((library, name)): Path<(String, String)>,
    axum::extract::Query(query): axum::extract::Query<Query>,
) -> Result<Html<String>> {
    let library = view::load(&engine, &library).await?;
    let field = field(&name).ok_or(scorarium_engine::Error::NotFound)?;
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
            Suggested::Role(role) => Some(Entry::value(role.as_str().to_owned())),
            Suggested::Key(text)
            | Suggested::TimeSignature(text)
            | Suggested::Instrumentation(text) => Some(Entry::value(text)),
            Suggested::Tag { tag, count } => Some(Entry {
                detail: Some(count.to_string()),
                ..Entry::value(tag.to_string())
            }),
            // These belong to fields this route never requests.
            Suggested::Publication(_)
            | Suggested::Work { .. }
            | Suggested::Publisher(_)
            | Suggested::Location(_) => None,
        })
        .collect();
    let fragment = Fragment {
        entries,
        create_person: matches!(field, SuggestField::Person).then(|| query.to_owned()),
    };
    Ok(Html(fragment.render()?))
}
