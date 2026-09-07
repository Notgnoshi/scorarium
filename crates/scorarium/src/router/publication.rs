use std::collections::BTreeSet;
use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum_extra::extract::Form as MultiForm;
use scorarium_archive::Library;
use sqlx::SqlitePool;

use super::{AppError, BaseContext, Crumb, FormFields, OrNotFound, RowEdit, Session};
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
    library: Library,
    publication: db::publication::Publication,
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
    let library = state.archive.library(library_id).await?.or_not_found()?;
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
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let Some(publication) = db::publication::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let works = db::work::list_in_publication(&state.pool, library_id, id).await?;
    let form = PublicationForm::stored(&publication, &works);
    render_edit(
        &state,
        base,
        library,
        publication,
        form,
        Errors::default(),
        &works,
    )
    .await
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
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let Some(publication) = db::publication::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let form = PublicationForm::from(submission);
    let validated = match form.parse() {
        Ok(validated) => validated,
        Err(errors) => {
            // The stored works are what the rejected rows' "and N more" counts come from
            let works = db::work::list_in_publication(&state.pool, library_id, id).await?;
            return render_edit(&state, base, library, publication, form, errors, &works).await;
        }
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
    library: Library,
    publication: db::publication::Publication,
    form: PublicationForm,
    errors: Errors,
    works: &[db::work::Work],
) -> Result<Response, AppError> {
    let contributors = works.iter().map(|w| (w.id, w.contributors.len())).collect();
    let page = EditPage {
        base: base.page(
            publication.title.clone(),
            vec![
                Crumb::home(),
                Crumb::library(&library),
                Crumb::publication(&publication),
            ],
        ),
        fields: FormFields::build(&state.pool, library.id, form, errors, &contributors)
            .await?
            .warn_when_empty(NO_COPIES)
            // A work row's edit button opens the work, which comes back here when it is done
            .edit_works(RowEdit::Stored {
                back: format!(
                    "/library/{}/publication/{}/edit",
                    library.id, publication.id
                ),
            }),
        library,
        publication,
    };
    Ok(Html(page.render()?).into_response())
}
