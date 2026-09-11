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
}

/// GET /library/{id}
pub async fn library(
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let library = base.visible_library(&state.archive, id).await?;
    let roles = library.roles().await?;
    let page = LibraryPage {
        base: base.page(library.name.clone(), vec![Crumb::home()]),
        publications: library.publications().await?,
        has_composers: roles.iter().any(|role| role == "composer"),
        has_authors: roles.iter().any(|role| role == "author"),
        library,
    };
    Ok(Html(page.render()?).into_response())
}

#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Visibility {
    Public,
    // A radio group always posts a value, but if one is missing, the safe choice is to hide
    #[default]
    Private,
}

impl Visibility {
    fn as_str(self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Private => "private",
        }
    }

    fn is_private(self) -> bool {
        self == Visibility::Private
    }

    fn from_private(private: bool) -> Self {
        if private {
            Visibility::Private
        } else {
            Visibility::Public
        }
    }
}

#[derive(Deserialize)]
pub struct LibraryForm {
    name: String,
    #[serde(default)]
    visibility: Visibility,
}

const EMPTY_NAME: &str = "The library name must not be empty.";

/// POST /library
pub async fn create(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Form(form): Form<LibraryForm>,
) -> Result<Response, AppError> {
    let name = form.name.trim();
    if name.is_empty() {
        return Ok(index::render(&state, base, Some(EMPTY_NAME))
            .await?
            .into_response());
    }
    state
        .archive
        .create_library(name, form.visibility.is_private())
        .await?;
    Ok(Redirect::to("/").into_response())
}

#[derive(Template)]
#[template(path = "library_edit.html")]
struct EditPage {
    base: BaseContext,
    library: Library,
    name: String,
    visibility: &'static str,
    error: Option<&'static str>,
}

/// GET /library/{id}/edit
pub async fn edit(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let library = state.archive.library(id).await?.or_not_found()?;
    let name = library.name.clone();
    let visibility = Visibility::from_private(library.private);
    render_edit(base, library, name, visibility, None)
}

/// POST /library/{id}/edit
pub async fn save(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path(id): Path<i64>,
    Form(form): Form<LibraryForm>,
) -> Result<Response, AppError> {
    let mut library = state.archive.library(id).await?.or_not_found()?;
    let name = form.name.trim();
    if name.is_empty() {
        return render_edit(
            base,
            library,
            String::new(),
            form.visibility,
            Some(EMPTY_NAME),
        );
    }
    library.update(name, form.visibility.is_private()).await?;
    Ok(Redirect::to(&format!("/library/{id}")).into_response())
}

fn render_edit(
    base: BaseContext,
    library: Library,
    name: String,
    visibility: Visibility,
    error: Option<&'static str>,
) -> Result<Response, AppError> {
    let page = EditPage {
        base: base.page(
            "Edit library",
            vec![Crumb::home(), Crumb::library(&library)],
        ),
        library,
        name,
        visibility: visibility.as_str(),
        error,
    };
    Ok(Html(page.render()?).into_response())
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
