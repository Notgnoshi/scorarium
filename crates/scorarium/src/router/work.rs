use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, RawForm, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_archive::{Library, Publication, Work, WorkErrors, WorkRawInput};
use serde::Deserialize;

use super::{AppError, BaseContext, Crumb, OrNotFound, Session, WorkFields};
use crate::{AppState, publication_post};

#[derive(Template)]
#[template(path = "work.html")]
struct WorkPage {
    base: BaseContext,
    work: Work,
    publications: Vec<Publication>,
}

#[derive(Template)]
#[template(path = "work_edit.html")]
struct EditPage {
    base: BaseContext,
    library: Library,
    work: Work,
    /// Where Save and Cancel lead
    back: String,
    fields: WorkFields,
}

/// A submitted work form, as the browser sends it. The stored work edit page and the draft work
/// page post the same shape.
#[derive(Deserialize)]
pub struct WorkPost {
    title: String,
    key: String,
    time_signature: String,
    instrumentation: String,
    // `default` covers a submission with no contributors at all
    #[serde(default)]
    contributor_name: Vec<String>,
    #[serde(default)]
    contributor_role: Vec<String>,
    #[serde(default)]
    catalog_number: Vec<String>,
}

impl From<WorkPost> for WorkRawInput {
    fn from(post: WorkPost) -> Self {
        WorkRawInput {
            // The page that submitted this names the work it belongs to, so the input need not
            id: None,
            title: post.title.trim().to_string(),
            key: post.key.trim().to_string(),
            time_signature: post.time_signature.trim().to_string(),
            instrumentation: post.instrumentation.trim().to_string(),
            contributors: publication_post::contributors(
                post.contributor_name,
                post.contributor_role,
            ),
            catalog_numbers: post
                .catalog_number
                .iter()
                .map(|number| number.trim().to_string())
                .collect(),
        }
    }
}

/// Which page opened the edit page, so Save and Cancel can return to it.
#[derive(Deserialize)]
pub struct BackQuery {
    back: Option<String>,
}

/// Where Save and Cancel lead: the page that opened this one, else the work page.
///
/// Only a path on this site is honored, so the parameter cannot send the user elsewhere. Browsers
/// read a backslash as a slash when parsing a URL, so "/\evil.example" is "//evil.example" to them
/// and is refused the same way.
fn back_to(library_id: i64, id: i64, back: Option<String>) -> String {
    back.filter(|back| back.starts_with('/') && !back[1..].starts_with(['/', '\\']))
        .unwrap_or_else(|| format!("/library/{library_id}/work/{id}"))
}

/// GET /library/{library_id}/work/{id}
pub async fn work(
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path((library_id, id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let work = library.work(id).await?.or_not_found()?;
    let publications = work.publications().await?;
    let page = WorkPage {
        base: base.page(
            work.title.clone(),
            vec![Crumb::home(), Crumb::library(&library)],
        ),
        work,
        publications,
    };
    Ok(Html(page.render()?).into_response())
}

/// GET /library/{library_id}/work/{id}/edit
pub async fn edit(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path((library_id, id)): Path<(i64, i64)>,
    Query(query): Query<BackQuery>,
) -> Result<Response, AppError> {
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let work = library.work(id).await?.or_not_found()?;
    let input = work.raw_input();
    render_edit(
        base,
        library,
        work,
        query.back,
        input,
        WorkErrors::default(),
    )
    .await
}

/// POST /library/{library_id}/work/{id}/edit
///
/// As on the publication edit page and unlike the import review page, there is no draft to fall
/// back on, so a rejected submission is rendered straight back with its messages.
pub async fn save(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path((library_id, id)): Path<(i64, i64)>,
    Query(query): Query<BackQuery>,
    RawForm(body): RawForm,
) -> Result<Response, AppError> {
    let post: WorkPost = publication_post::decode_form(&body)?;
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let mut work = library.work(id).await?.or_not_found()?;
    let input = WorkRawInput::from(post);
    match input.parse() {
        Ok(parsed) => {
            work.update(&parsed).await?;
            // A merge folded this work into an older one; show the survivor rather than a page for
            // a work that no longer exists
            let next = if work.id == id {
                back_to(library_id, id, query.back)
            } else {
                format!("/library/{library_id}/work/{}", work.id)
            };
            Ok(Redirect::to(&next).into_response())
        }
        Err(errors) => render_edit(base, library, work, query.back, input, errors).await,
    }
}

async fn render_edit(
    base: BaseContext,
    library: Library,
    work: Work,
    back: Option<String>,
    input: WorkRawInput,
    errors: WorkErrors,
) -> Result<Response, AppError> {
    let fields = WorkFields::build(&library, input, errors).await?;
    let page = EditPage {
        base: base.page(
            work.title.clone(),
            vec![Crumb::home(), Crumb::library(&library), Crumb::work(&work)],
        ),
        back: back_to(library.id, work.id, back),
        fields,
        library,
        work,
    };
    Ok(Html(page.render()?).into_response())
}
