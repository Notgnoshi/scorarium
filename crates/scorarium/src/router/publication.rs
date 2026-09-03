use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum_extra::extract::CookieJar;

use super::{AppError, BaseContext, Crumb};
use crate::{AppState, db};

#[derive(Template)]
#[template(path = "publication.html")]
struct PublicationPage {
    base: BaseContext,
    publication: db::publication::Publication,
}

/// GET /library/{library_id}/publication/{id}
pub async fn publication(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path((library_id, id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    let Some(library) = db::get_library(&state.pool, library_id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let Some(publication) = db::publication::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let page = PublicationPage {
        base: BaseContext::new(
            publication.title.clone(),
            format!("/library/{library_id}/publication/{id}"),
            super::logged_in(&state, &jar),
            vec![Crumb::home(), Crumb::library(&library)],
        ),
        publication,
    };
    Ok(Html(page.render()?).into_response())
}
