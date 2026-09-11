use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use scorarium_archive::{CatalogNumber, CatalogNumberEntry, Similarity};
use serde::{Deserialize, Serialize};

use super::{AppError, OrNotFound, Session};
use crate::AppState;

/// What the form has typed so far, and the composer it credits the work to
#[derive(Deserialize)]
pub struct SuggestQuery {
    #[serde(default)]
    q: String,
    composer: Option<String>,
}

#[derive(Serialize)]
pub struct Suggestions {
    /// Whether what was typed is itself a recognized number, for the form's indicator
    recognized: bool,
    matches: Vec<Match>,
}

#[derive(Serialize)]
struct Match {
    value: String,
    title: String,
    composer: Option<String>,
    exact: bool,
}

/// How many suggestions a dropdown can show
const LIMIT: usize = 10;

/// GET /library/{id}/suggest/catalog-numbers
///
/// Requires being logged in
pub async fn catalog_numbers(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path(library_id): Path<i64>,
    Query(query): Query<SuggestQuery>,
) -> Result<Json<Suggestions>, AppError> {
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let typed = CatalogNumber::parse(&query.q);
    let composer = query
        .composer
        .as_deref()
        .map(str::trim)
        .filter(|composer| !composer.is_empty());
    let mut ranked: Vec<(Similarity, CatalogNumberEntry)> = library
        .catalog_numbers(composer)
        .await?
        .into_iter()
        .filter_map(|entry| {
            entry
                .number
                .similarity(&typed)
                .map(|similarity| (similarity, entry))
        })
        .collect();
    ranked.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.number.cmp(&b.1.number))
            .then_with(|| a.1.title.cmp(&b.1.title))
    });
    // A work credited to two composers comes back once per composer
    ranked.dedup_by(|a, b| a.1.work_id == b.1.work_id && a.1.number == b.1.number);
    ranked.truncate(LIMIT);
    Ok(Json(Suggestions {
        recognized: typed.is_recognized(),
        matches: ranked
            .into_iter()
            .map(|(similarity, entry)| Match {
                value: entry.number.as_str().to_string(),
                title: entry.title,
                composer: entry.composer,
                exact: similarity == Similarity::Exact,
            })
            .collect(),
    }))
}
