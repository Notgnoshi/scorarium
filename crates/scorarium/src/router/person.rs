use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

use super::{AppError, BaseContext, Crumb};
use crate::{AppState, db};

#[derive(Template)]
#[template(path = "person.html")]
struct PersonPage {
    base: BaseContext,
    person: db::person::Person,
    /// Each publication the person is credited on, with only this person's works from it.
    publications: Vec<(db::publication::Publication, Vec<db::work::Work>)>,
}

/// GET /library/{library_id}/person/{id}
pub async fn person(
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path((library_id, id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    let Some(library) = db::get_library(&state.pool, library_id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let Some(person) = db::person::get(&state.pool, library_id, id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let mut publications = db::publication::list_by_person(&state.pool, library_id, id).await?;
    publications.sort_by(|a, b| a.title.cmp(&b.title));
    let mut nested = Vec::with_capacity(publications.len());
    for publication in publications {
        let works = db::work::list_in_publication(&state.pool, library_id, publication.id)
            .await?
            .into_iter()
            .filter(|w| w.contributors.iter().any(|c| c.person_id == id))
            .collect();
        nested.push((publication, works));
    }
    let page = PersonPage {
        base: base.page(
            person.name.clone(),
            vec![Crumb::home(), Crumb::library(&library)],
        ),
        person,
        publications: nested,
    };
    Ok(Html(page.render()?).into_response())
}

#[derive(Template)]
#[template(path = "persons.html")]
struct PersonsPage {
    base: BaseContext,
    persons: Vec<db::person::Person>,
}

/// GET /library/{id}/composers
pub async fn composers(
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    listing(&state, base, id, "composer", "Composers").await
}

/// GET /library/{id}/authors
pub async fn authors(
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    listing(&state, base, id, "author", "Authors").await
}

async fn listing(
    state: &AppState,
    base: BaseContext,
    library_id: i64,
    role: &str,
    title: &str,
) -> Result<Response, AppError> {
    let Some(library) = db::get_library(&state.pool, library_id).await? else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let page = PersonsPage {
        base: base.page(title, vec![Crumb::home(), Crumb::library(&library)]),
        persons: db::person::list_with_role(&state.pool, library_id, role).await?,
    };
    Ok(Html(page.render()?).into_response())
}
