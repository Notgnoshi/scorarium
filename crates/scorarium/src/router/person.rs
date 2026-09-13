use std::sync::Arc;

use askama::Template;
use axum::extract::{Path, RawForm, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_archive::{Library, Person, PersonErrors, PersonRawInput, Publication, Work};
use serde::Deserialize;

use super::{AppError, BaseContext, Crumb, OrNotFound, Session, pair_messages};
use crate::{AppState, publication_post};

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
#[template(path = "person_edit.html")]
struct EditPage {
    base: BaseContext,
    library: Library,
    person: Person,
    fields: PersonFields,
}

/// Everything the person form renders
struct PersonFields {
    input: PersonRawInput,
    errors: PersonErrors,
    links: Vec<(String, String)>,
}

impl PersonFields {
    fn build(input: PersonRawInput, errors: PersonErrors) -> Self {
        Self {
            links: pair_messages(&input.links, &errors.links),
            input,
            errors,
        }
    }
}

/// A submitted person form, as the browser sends it.
#[derive(Deserialize)]
pub struct PersonPost {
    name: String,
    #[serde(default)]
    link: Vec<String>,
}

impl From<PersonPost> for PersonRawInput {
    fn from(post: PersonPost) -> Self {
        PersonRawInput {
            name: post.name.trim().to_string(),
            links: post
                .link
                .iter()
                .map(|link| link.trim().to_string())
                .collect(),
        }
    }
}

/// GET /library/{library_id}/person/{id}/edit
pub async fn edit(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path((library_id, id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    // A logged-in user sees private libraries, so this does not go through visible_library
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let person = library.person(id).await?.or_not_found()?;
    let input = person.raw_input();
    render_edit(base, library, person, input, PersonErrors::default())
}

/// POST /library/{library_id}/person/{id}/edit
pub async fn save(
    _session: Session,
    State(state): State<Arc<AppState>>,
    base: BaseContext,
    Path((library_id, id)): Path<(i64, i64)>,
    RawForm(body): RawForm,
) -> Result<Response, AppError> {
    let post: PersonPost = publication_post::decode_form(&body)?;
    let library = state.archive.library(library_id).await?.or_not_found()?;
    let mut person = library.person(id).await?.or_not_found()?;
    let input = PersonRawInput::from(post);
    match input.parse() {
        Ok(parsed) => {
            person.update(&parsed).await?;
            Ok(Redirect::to(&format!("/library/{library_id}/person/{id}")).into_response())
        }
        Err(errors) => render_edit(base, library, person, input, errors),
    }
}

fn render_edit(
    base: BaseContext,
    library: Library,
    person: Person,
    input: PersonRawInput,
    errors: PersonErrors,
) -> Result<Response, AppError> {
    let page = EditPage {
        base: base.page(
            person.name.clone(),
            vec![
                Crumb::home(),
                Crumb::library(&library),
                Crumb::person(&person),
            ],
        ),
        fields: PersonFields::build(input, errors),
        library,
        person,
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
