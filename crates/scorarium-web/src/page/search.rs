use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::EntityKind;
use scorarium_engine::value::Classification;
use serde::Deserialize;

use crate::auth::Viewer;
use crate::error::Result;
use crate::hit::Hit;
use crate::page::Base;
use crate::widget::header::Header;

#[derive(Deserialize)]
pub(crate) struct Query {
    q: Option<String>,
}

#[derive(Template)]
#[template(path = "page/search.html")]
struct Search {
    base: Base,
    logged_in: bool,
    query: String,
    hits: Vec<Hit>,
    total_hits: usize,
}

const LIMIT: usize = 50;

pub async fn get(
    viewer: Viewer,
    State(engine): State<Engine>,
    mut header: Header,
    axum::extract::Query(query): axum::extract::Query<Query>,
) -> Result<Response> {
    let query = query.q.as_deref().unwrap_or_default().trim().to_owned();
    let (title, results) = if query.is_empty() {
        ("Search".to_owned(), Default::default())
    } else {
        let results = engine.search(&query, viewer.is_logged_in(), LIMIT).await?;
        (format!("Search results for \"{query}\""), results)
    };
    header.query = query.clone();
    let page = Search {
        base: Base::new(header, title, Vec::new()),
        logged_in: viewer.is_logged_in(),
        query,
        hits: results.hits.iter().map(Into::into).collect(),
        total_hits: results.total_hits,
    };
    Ok(Html(page.render()?).into_response())
}
