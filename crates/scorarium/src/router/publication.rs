use std::collections::BTreeSet;
use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, RawForm, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_archive::{Library, Publication, PublicationErrors, PublicationRawInput, Work};

use super::{AppError, BaseContext, Crumb, FormFields, OrNotFound, Session, WorkEdit};
use crate::AppState;
use crate::publication_post::PublicationPost;

#[derive(Template)]
#[template(path = "publication.html")]
struct PublicationPage {
    base: BaseContext,
    publication: Publication,
    works: Vec<Work>,
    show_catalog_numbers: bool,
    roles: Vec<String>,
}

#[derive(Template)]
#[template(path = "publication_edit.html")]
struct EditPage {
    base: BaseContext,
    library: Library,
    publication: Publication,
    fields: FormFields,
}

const NO_COPIES: &str = "Removing the last copy will delete this publication and its contents.";

/// The works table and its columns. Columns that would be empty for every work are left out:
/// books have no catalog numbers, and their works have authors where scores have composers.
fn works_table(works: Vec<Work>) -> (Vec<Work>, bool, Vec<String>) {
    let show_catalog_numbers = works.iter().any(|work| !work.catalog_numbers.is_empty());
    let roles = works
        .iter()
        .flat_map(|work| &work.contributors)
        .map(|contributor| contributor.role.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    (works, show_catalog_numbers, roles)
}

/// GET /library/{library_id}/publication/{id}
pub async fn publication(
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path((library_id, id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    let library = base.visible_library(&state.archive, library_id).await?;
    let publication = library.publication(id).await?.or_not_found()?;
    let (works, show_catalog_numbers, roles) = works_table(publication.works().await?);
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
    let publication = library.publication(id).await?.or_not_found()?;
    let input = publication.raw_input(&publication.works().await?);
    render_edit(
        base,
        library,
        publication,
        input,
        PublicationErrors::default(),
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
    RawForm(body): RawForm,
) -> Result<Response, AppError> {
    let post = PublicationPost::decode(&body)?;
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let mut publication = library.publication(id).await?.or_not_found()?;
    // The page showed one contributor per work; the stored works are what the rest comes from
    let shown = publication.raw_input(&publication.works().await?).contents;
    let input = post.merge(shown);
    match input.parse() {
        Ok(parsed) => {
            publication.update(&parsed).await?;
            Ok(Redirect::to(&format!("/library/{library_id}/publication/{id}")).into_response())
        }
        Err(errors) => render_edit(base, library, publication, input, errors).await,
    }
}

/// POST /library/{library_id}/publication/{id}/delete
pub async fn delete(
    _session: Session,
    State(state): State<Arc<AppState>>,
    Path((library_id, id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    let library = state.archive.library(library_id).await?.or_not_found()?;
    library
        .publication(id)
        .await?
        .or_not_found()?
        .delete()
        .await?;
    Ok(Redirect::to(&format!("/library/{library_id}")).into_response())
}

async fn render_edit(
    base: BaseContext,
    library: Library,
    publication: Publication,
    input: PublicationRawInput,
    errors: PublicationErrors,
) -> Result<Response, AppError> {
    let fields = FormFields::build(&library, input, errors)
        .await?
        .warn_when_empty(NO_COPIES)
        // A work's edit button opens the work, which comes back here when it is done
        .edit_works(WorkEdit::Stored {
            back: format!(
                "/library/{}/publication/{}/edit",
                library.id, publication.id
            ),
        });
    let page = EditPage {
        base: base.page(
            publication.title.clone(),
            vec![
                Crumb::home(),
                Crumb::library(&library),
                Crumb::publication(&publication),
            ],
        ),
        fields,
        library,
        publication,
    };
    Ok(Html(page.render()?).into_response())
}
