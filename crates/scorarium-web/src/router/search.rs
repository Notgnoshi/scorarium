use std::sync::Arc;

use askama::Template;
use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::archive::{Entity, PersonSummary, WorkSummary};
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
        Entity::Person(person) => ("person", person.name.clone(), person_credit(person)),
        Entity::Work(work) => (
            "work",
            work.title.clone(),
            credit(
                work.numbers.first().map(String::as_str),
                contributor_name(work),
            ),
        ),
        Entity::Publication(publication) => (
            "publication",
            publication.title.clone(),
            publication.people.join(", "),
        ),
    }
}

/// Get the primary and secondary descriptions for a catalog number
pub(crate) fn describe_number(work: &WorkSummary, number: &str) -> (String, String) {
    (
        number.to_string(),
        credit(Some(&work.title), contributor_name(work)),
    )
}

fn contributor_name(work: &WorkSummary) -> Option<&str> {
    work.contributor.as_ref().map(|person| person.name.as_str())
}

/// "Op. 9 No. 2 by Frederic Chopin", or whichever half exists
pub(crate) fn credit(what: Option<&str>, by: Option<&str>) -> String {
    match (what, by) {
        (Some(what), Some(by)) => format!("{what} by {by}"),
        (Some(what), None) => what.to_string(),
        (None, Some(by)) => format!("by {by}"),
        (None, None) => String::new(),
    }
}

pub(crate) fn person_credit(person: &PersonSummary) -> String {
    match person.others {
        0 => person.title.clone(),
        others => format!("{} +{others}", person.title),
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
