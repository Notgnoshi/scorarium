use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum_extra::extract::CookieJar;

use super::{AppError, BaseContext, Crumb};
use crate::{AppState, db};

#[derive(Template)]
#[template(path = "work.html")]
struct WorkPage {
    base: BaseContext,
    work: db::work::Work,
    publications: Vec<db::publication::Publication>,
}

/// GET /library/{library_id}/work/{id}
pub async fn work(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path((library_id, id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    let Some(library) = db::get_library(&state.pool, library_id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let Some(work) = db::work::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let publications = db::publication::list_containing(&state.pool, library_id, id).await?;
    let page = WorkPage {
        base: BaseContext::new(
            work.title.clone(),
            format!("/library/{library_id}/work/{id}"),
            super::logged_in(&state, &jar),
            vec![Crumb::home(), Crumb::library(&library)],
        ),
        work,
        publications,
    };
    Ok(Html(page.render()?).into_response())
}
