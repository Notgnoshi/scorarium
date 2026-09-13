use std::sync::Arc;

use askama::Template;
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Response};
use scorarium_archive::{Entity, WorkSummary};
use serde::Deserialize;

use super::{AppError, BaseContext, Crumb};
use crate::AppState;

#[derive(Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    q: String,
}

/// One hit as the page shows it
struct ShownHit {
    kind: &'static str,
    href: String,
    primary: String,
    secondary: String,
    library_name: String,
}

#[derive(Template)]
#[template(path = "search.html")]
struct SearchPage {
    base: BaseContext,
    hits: Vec<ShownHit>,
}

/// The kind and the two lines of text a summary shows wherever it is listed. Shared with the
/// suggestion handlers, so a search hit and a dropdown item read the same.
pub(crate) fn describe(entity: &Entity) -> (&'static str, String, String) {
    match entity {
        Entity::Person(person) => (
            "person",
            person.name.clone(),
            match person.works {
                1 => "1 work".to_string(),
                works => format!("{works} works"),
            },
        ),
        Entity::Work(work) => (
            "work",
            work.title.clone(),
            credit(work.numbers.first(), work),
        ),
        Entity::Publication(publication) => (
            "publication",
            publication.title.clone(),
            publication.people.join(", "),
        ),
    }
}

/// "Op. 9 No. 2 by Frederic Chopin", or whichever half exists
fn credit(what: Option<&String>, work: &WorkSummary) -> String {
    let by = work
        .contributor
        .as_ref()
        .map(|person| format!("by {}", person.name));
    match (what, by) {
        (Some(what), Some(by)) => format!("{what} {by}"),
        (Some(what), None) => what.clone(),
        (None, Some(by)) => by,
        (None, None) => String::new(),
    }
}

pub(crate) fn href(library_id: i64, entity: &Entity) -> String {
    let (kind, id) = match entity {
        Entity::Person(person) => ("person", person.id),
        Entity::Work(work) => ("work", work.id),
        Entity::Publication(publication) => ("publication", publication.id),
    };
    format!("/library/{library_id}/{kind}/{id}")
}

/// GET /search
pub async fn search(
    State(state): State<Arc<AppState>>,
    mut base: BaseContext,
    Query(query): Query<SearchQuery>,
) -> Result<Response, AppError> {
    let typed = query.q.trim().to_string();
    let hits = state
        .archive
        .search(&typed, !base.logged_in)
        .await?
        .into_iter()
        .map(|hit| {
            let (kind, primary, secondary) = describe(&hit.entity);
            ShownHit {
                kind,
                href: href(hit.library_id, &hit.entity),
                primary,
                secondary,
                library_name: hit.library_name,
            }
        })
        .collect();
    base.search_query = typed;
    let page = SearchPage {
        base: base.page("Search", vec![Crumb::home()]),
        hits,
    };
    Ok(Html(page.render()?).into_response())
}
