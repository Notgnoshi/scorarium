use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::Form as MultiForm;
use scorarium_archive::Library;
use serde::Deserialize;

use super::{AppError, BaseContext, Crumb, OrNotFound, Session, WorkFields};
use crate::work_form::{Errors, Submission, WorkForm};
use crate::{AppState, db};

#[derive(Template)]
#[template(path = "work.html")]
struct WorkPage {
    base: BaseContext,
    work: db::work::Work,
    publications: Vec<db::publication::Publication>,
}

#[derive(Template)]
#[template(path = "work_edit.html")]
struct EditPage {
    base: BaseContext,
    library: Library,
    work: db::work::Work,
    /// Where Save and Cancel lead
    back: String,
    fields: WorkFields,
}

/// Which page opened the edit page, so Save and Cancel can return to it.
#[derive(Deserialize)]
pub struct BackQuery {
    back: Option<String>,
}

/// Where Save and Cancel lead: the page that opened this one, else the work page.
fn back_to(library_id: i64, id: i64, back: Option<String>) -> String {
    back.filter(|back| back.starts_with(&format!("/library/{library_id}/")))
        .unwrap_or_else(|| format!("/library/{library_id}/work/{id}"))
}

/// GET /library/{library_id}/work/{id}
pub async fn work(
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path((library_id, id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let Some(work) = db::work::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let publications = db::publication::list_containing(&state.pool, library_id, id).await?;
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
    let Some(work) = db::work::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let form = WorkForm::stored(&work);
    render_edit(
        &state,
        base,
        library,
        work,
        query.back,
        form,
        Errors::default(),
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
    MultiForm(submission): MultiForm<Submission>,
) -> Result<Response, AppError> {
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let Some(work) = db::work::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let form = WorkForm::from(submission);
    let update = match form.parse() {
        Ok(update) => update,
        Err(errors) => {
            return render_edit(&state, base, library, work, query.back, form, errors).await;
        }
    };
    if !db::work::update(&state.pool, library_id, id, &update).await? {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    Ok(Redirect::to(&back_to(library_id, id, query.back)).into_response())
}

async fn render_edit(
    state: &AppState,
    base: BaseContext,
    library: Library,
    work: db::work::Work,
    back: Option<String>,
    form: WorkForm,
    errors: Errors,
) -> Result<Response, AppError> {
    let page = EditPage {
        base: base.page(
            work.title.clone(),
            vec![Crumb::home(), Crumb::library(&library), Crumb::work(&work)],
        ),
        fields: WorkFields::build(&state.pool, library.id, form, errors).await?,
        back: back_to(library.id, work.id, back),
        library,
        work,
    };
    Ok(Html(page.render()?).into_response())
}
