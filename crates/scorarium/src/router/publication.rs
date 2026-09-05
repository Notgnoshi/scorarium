use std::collections::BTreeSet;
use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

use super::{AppError, BaseContext, Crumb};
use crate::{AppState, db};

#[derive(Template)]
#[template(path = "publication.html")]
struct PublicationPage {
    base: BaseContext,
    publication: db::publication::Publication,
    works: Vec<db::work::Work>,
    // Columns that would be empty for every work are left out: books have no catalog numbers, and
    // their works have authors where scores have composers.
    show_catalog_numbers: bool,
    /// The distinct contributor roles across the works, one column each.
    roles: Vec<String>,
}

/// GET /library/{library_id}/publication/{id}
pub async fn publication(
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path((library_id, id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    let Some(library) = db::get_library(&state.pool, library_id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let Some(publication) = db::publication::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let works = db::work::list_in_publication(&state.pool, library_id, id).await?;
    let page = PublicationPage {
        base: base.page(
            publication.title.clone(),
            vec![Crumb::home(), Crumb::library(&library)],
        ),
        show_catalog_numbers: works.iter().any(|w| !w.catalog_numbers.is_empty()),
        roles: works
            .iter()
            .flat_map(|w| &w.contributors)
            .map(|c| c.role.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        publication,
        works,
    };
    Ok(Html(page.render()?).into_response())
}
