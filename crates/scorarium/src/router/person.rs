use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Response};
use scorarium_archive::{Person, Publication, Work};

use super::{AppError, BaseContext, Crumb, OrNotFound};
use crate::AppState;

#[derive(Template)]
#[template(path = "person.html")]
struct PersonPage {
    base: BaseContext,
    person: Person,
    /// Each publication the person is credited on, with only this person's works from it.
    publications: Vec<(Publication, Vec<Work>)>,
}

/// GET /library/{library_id}/person/{id}
pub async fn person(
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path((library_id, id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    let library = base.visible_library(&state.archive, library_id).await?;
    let person = library.person(id).await?.or_not_found()?;
    let mut publications = person.publications().await?;
    publications.sort_by(|a, b| a.title.cmp(&b.title));
    let mut nested = Vec::with_capacity(publications.len());
    for publication in publications {
        // A publication may credit the person directly and contain works that do not
        let works = publication
            .works()
            .await?
            .into_iter()
            .filter(|work| !work.roles_of(id).is_empty())
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
    persons: Vec<Person>,
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
    let library = base.visible_library(&state.archive, library_id).await?;
    let page = PersonsPage {
        base: base.page(title, vec![Crumb::home(), Crumb::library(&library)]),
        persons: library.persons_with_role(role).await?,
    };
    Ok(Html(page.render()?).into_response())
}
