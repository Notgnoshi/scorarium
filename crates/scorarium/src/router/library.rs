use std::sync::Arc;

use askama::Template;
use axum::Form;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_archive::{Library, Publication};
use serde::Deserialize;

use super::{AppError, BaseContext, Crumb, OrNotFound, Session, index};
use crate::AppState;

#[derive(Template)]
#[template(path = "library.html")]
struct LibraryPage {
    base: BaseContext,
    library: Library,
    publications: Vec<Publication>,
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
    render(&state, id, base, None).await
}

async fn render(
    state: &AppState,
    id: i64,
    base: BaseContext,
    error: Option<&'static str>,
) -> Result<Response, AppError> {
    let library = base.visible_library(&state.archive, id).await?;
    let roles = library.roles().await?;
    let page = LibraryPage {
        base: base.page(library.name.clone(), vec![Crumb::home()]),
        publications: library.publications().await?,
        has_composers: roles.iter().any(|role| role == "composer"),
        has_authors: roles.iter().any(|role| role == "author"),
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
        return Ok(index::render(&state, base, Some(EMPTY_NAME))
            .await?
            .into_response());
    }
    state.archive.create_library(name, true).await?;
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
        return render(&state, id, base, Some(EMPTY_NAME)).await;
    }
    let mut library = state.archive.library(id).await?.or_not_found()?;
    library.rename(name).await?;
    Ok(Redirect::to(&format!("/library/{id}")).into_response())
}

/// POST /library/{id}/delete
pub async fn delete(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let library = state.archive.library(id).await?.or_not_found()?;
    library.delete().await?;
    Ok(Redirect::to("/").into_response())
}
