use askama::Template;
use axum::extract::State;
use axum::response::Html;
use scorarium_engine::Engine;

use crate::api::{Query, SUGGESTION_LIMIT};
use crate::auth::Viewer;
use crate::error::Result;
use crate::hit::Hit;

#[derive(Template)]
#[template(path = "api/search.html")]
struct Fragment {
    query: String,
    hits: Vec<Hit>,
}

pub async fn get(
    viewer: Viewer,
    State(engine): State<Engine>,
    axum::extract::Query(query): axum::extract::Query<Query>,
) -> Result<Html<String>> {
    let query = query.q.as_deref().unwrap_or_default().trim();
    if query.is_empty() {
        return Ok(Html(String::new()));
    }
    let results = engine
        .search(query, viewer.is_logged_in(), SUGGESTION_LIMIT)
        .await?;
    let fragment = Fragment {
        query: query.to_owned(),
        hits: results.hits.iter().map(Into::into).collect(),
    };
    Ok(Html(fragment.render()?))
}
