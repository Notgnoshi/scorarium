use std::sync::Arc;

use askama::Template;
use axum::Form;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use sqlx::SqlitePool;

use super::{AppError, BaseContext, Crumb, Session, index};
use crate::{AppState, db};

#[derive(Template)]
#[template(path = "library.html")]
struct LibraryPage {
    base: BaseContext,
    library: db::Library,
    publications: Vec<db::publication::Publication>,
    // Link only to listings with something in them
    has_composers: bool,
    has_authors: bool,
    error: Option<&'static str>,
}

/// GET /library/{id}
pub async fn library(
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    render(&state.pool, id, base, None).await
}

async fn render(
    pool: &SqlitePool,
    id: i64,
    base: BaseContext,
    error: Option<&'static str>,
) -> Result<Response, AppError> {
    let Some(library) = db::get_library(pool, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let roles = db::person::list_roles(pool, id).await?;
    let page = LibraryPage {
        base: base.page(library.name.clone(), vec![Crumb::home()]),
        publications: db::publication::list(pool, id).await?,
        has_composers: roles.iter().any(|r| r == "composer"),
        has_authors: roles.iter().any(|r| r == "author"),
        library,
        error,
    };
    Ok(Html(page.render()?).into_response())
}

#[derive(Deserialize)]
pub struct NameForm {
    name: String,
}

const EMPTY_NAME: &str = "The library name must not be empty.";

/// POST /library
pub async fn create(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Form(form): Form<NameForm>,
) -> Result<Response, AppError> {
    let name = form.name.trim();
    if name.is_empty() {
        return Ok(index::render(&state.pool, base, Some(EMPTY_NAME))
            .await?
            .into_response());
    }
    db::create_library(&state.pool, name).await?;
    Ok(Redirect::to("/").into_response())
}

/// POST /library/{id}/rename
pub async fn rename(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path(id): Path<i64>,
    Form(form): Form<NameForm>,
) -> Result<Response, AppError> {
    let name = form.name.trim();
    if name.is_empty() {
        return render(&state.pool, id, base, Some(EMPTY_NAME)).await;
    }
    if !db::rename_library(&state.pool, id, name).await? {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    Ok(Redirect::to(&format!("/library/{id}")).into_response())
}

/// POST /library/{id}/delete
pub async fn delete(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    if !db::delete_library(&state.pool, id).await? {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    Ok(Redirect::to("/").into_response())
}
