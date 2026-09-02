use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum_extra::extract::CookieJar;

use super::{AppError, BaseContext, Crumb};
use crate::{AppState, db};

#[derive(Template)]
#[template(path = "library.html")]
struct LibraryPage {
    base: BaseContext,
    library: db::Library,
}

/// GET /library/{id}
pub async fn library(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let Some(library) = db::get_library(&state.pool, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let page = LibraryPage {
        base: BaseContext::new(
            library.name.clone(),
            format!("/library/{id}"),
            super::logged_in(&state, &jar),
            vec![Crumb::home()],
        ),
        library,
    };
    Ok(Html(page.render()?).into_response())
}
