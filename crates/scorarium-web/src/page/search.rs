use std::cmp::Ordering;

use askama::Template;
use axum::extract::State;
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use serde::Deserialize;

use crate::auth::Viewer;
use crate::error::Result;
use crate::hit::Hit;
use crate::page::Base;
use crate::sort;
use crate::widget::header::Header;

#[derive(Deserialize)]
pub(crate) struct Query {
    q: Option<String>,
    sort: Option<Sort>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Column {
    /// The order the search engine ranked the hits in
    #[default]
    Relevance,
    Title,
    Library,
    Kind,
}

impl sort::Column for Column {
    const ALL: &'static [Self] = &[
        Column::Relevance,
        Column::Title,
        Column::Library,
        Column::Kind,
    ];

    fn name(self) -> &'static str {
        match self {
            Column::Relevance => "relevance",
            Column::Title => "title",
            Column::Library => "library",
            Column::Kind => "kind",
        }
    }
}

type Sort = sort::Sort<Column>;

/// Sort hits by the column, title breaking ties, or leave them in relevance order
fn sort_hits(hits: &mut [Hit], sort: &Sort) {
    if sort.column == Column::Relevance {
        return;
    }
    hits.sort_by(|a, b| {
        let (a_title, b_title) = (a.title.to_lowercase(), b.title.to_lowercase());
        let by_column = match sort.column {
            Column::Relevance => Ordering::Equal,
            Column::Title => sort::compare(Some(&a_title), Some(&b_title), sort.descending),
            Column::Library => sort::compare(
                Some(a.library_name.to_lowercase()),
                Some(b.library_name.to_lowercase()),
                sort.descending,
            ),
            Column::Kind => {
                sort::compare(Some(a.kind_rank()), Some(b.kind_rank()), sort.descending)
            }
        };
        by_column.then_with(|| a_title.cmp(&b_title))
    });
}

#[derive(Template)]
#[template(path = "page/search.html")]
struct Search {
    base: Base,
    logged_in: bool,
    query: String,
    hits: Vec<Hit>,
    total_hits: usize,
    sort: Sort,
}

const LIMIT: usize = 50;

pub async fn get(
    viewer: Viewer,
    State(engine): State<Engine>,
    mut header: Header,
    axum::extract::Query(params): axum::extract::Query<Query>,
) -> Result<Response> {
    let query = params.q.as_deref().unwrap_or_default().trim().to_owned();
    let (title, results) = if query.is_empty() {
        ("Search".to_owned(), Default::default())
    } else {
        let results = engine.search(&query, viewer.is_logged_in(), LIMIT).await?;
        (format!("Search results for \"{query}\""), results)
    };
    header.query = query.clone();
    let mut hits: Vec<Hit> = results.hits.iter().map(Into::into).collect();
    let sort = params.sort.unwrap_or_default().keep("q", &query);
    sort_hits(&mut hits, &sort);
    let page = Search {
        base: Base::new(header, title, Vec::new()),
        logged_in: viewer.is_logged_in(),
        query,
        hits,
        total_hits: results.total_hits,
        sort,
    };
    Ok(Html(page.render()?).into_response())
}
