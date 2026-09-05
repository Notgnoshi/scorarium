use std::collections::BTreeSet;
use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::Form as MultiForm;
use sqlx::SqlitePool;

use super::{AppError, BaseContext, Crumb, FormFields, Session};
use crate::publication_form::{Errors, PublicationForm, Submission};
use crate::{AppState, db};

#[derive(Template)]
#[template(path = "publication.html")]
struct PublicationPage {
    base: BaseContext,
    publication: db::publication::Publication,
    works: Vec<db::work::Work>,
    show_catalog_numbers: bool,
    roles: Vec<String>,
}

#[derive(Template)]
#[template(path = "publication_edit.html")]
struct EditPage {
    base: BaseContext,
    library: db::Library,
    publication: db::publication::Publication,
    works: Vec<db::work::Work>,
    show_catalog_numbers: bool,
    roles: Vec<String>,
    fields: FormFields,
}

const NO_COPIES: &str = "Removing the last copy will delete this publication and its contents.";

/// The works table and its columns. Columns that would be empty for every work are left out:
/// books have no catalog numbers, and their works have authors where scores have composers.
async fn works_table(
    pool: &SqlitePool,
    library_id: i64,
    publication_id: i64,
) -> sqlx::Result<(Vec<db::work::Work>, bool, Vec<String>)> {
    let works = db::work::list_in_publication(pool, library_id, publication_id).await?;
    let show_catalog_numbers = works.iter().any(|w| !w.catalog_numbers.is_empty());
    let roles = works
        .iter()
        .flat_map(|w| &w.contributors)
        .map(|c| c.role.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok((works, show_catalog_numbers, roles))
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
    let (works, show_catalog_numbers, roles) = works_table(&state.pool, library_id, id).await?;
    let page = PublicationPage {
        base: base.page(
            publication.title.clone(),
            vec![Crumb::home(), Crumb::library(&library)],
        ),
        publication,
        works,
        show_catalog_numbers,
        roles,
    };
    Ok(Html(page.render()?).into_response())
}

/// GET /library/{library_id}/publication/{id}/edit
pub async fn edit(
    _session: Session,
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
    let form = PublicationForm::from(&publication);
    render_edit(&state, base, library, publication, form, Errors::default()).await
}

/// POST /library/{library_id}/publication/{id}/edit
///
/// Unlike the import review page there is no draft to fall back on, so a rejected submission is
/// rendered straight back with its messages.
pub async fn save(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path((library_id, id)): Path<(i64, i64)>,
    MultiForm(submission): MultiForm<Submission>,
) -> Result<Response, AppError> {
    let Some(library) = db::get_library(&state.pool, library_id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let Some(publication) = db::publication::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let form = PublicationForm::from(submission);
    let validated = match form.parse() {
        Ok(validated) => validated,
        Err(errors) => return render_edit(&state, base, library, publication, form, errors).await,
    };
    if !db::publication::update(&state.pool, library_id, id, &validated).await? {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    Ok(Redirect::to(&format!("/library/{library_id}/publication/{id}")).into_response())
}

/// POST /library/{library_id}/publication/{id}/delete
pub async fn delete(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path((library_id, id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    if !db::publication::delete(&state.pool, library_id, id).await? {
        return Ok(StatusCode::NOT_FOUND.into_response());
    }
    Ok(Redirect::to(&format!("/library/{library_id}")).into_response())
}

async fn render_edit(
    state: &AppState,
    base: BaseContext,
    library: db::Library,
    publication: db::publication::Publication,
    form: PublicationForm,
    errors: Errors,
) -> Result<Response, AppError> {
    let (works, show_catalog_numbers, roles) =
        works_table(&state.pool, library.id, publication.id).await?;
    let page = EditPage {
        base: base.page(
            publication.title.clone(),
            vec![
                Crumb::home(),
                Crumb::library(&library),
                Crumb::publication(&publication),
            ],
        ),
        fields: FormFields::build(&state.pool, library.id, form, errors)
            .await?
            .warn_when_empty(NO_COPIES),
        works,
        show_catalog_numbers,
        roles,
        library,
        publication,
    };
    Ok(Html(page.render()?).into_response())
}
