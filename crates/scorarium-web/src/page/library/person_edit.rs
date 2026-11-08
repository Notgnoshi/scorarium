use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::library::{Library, LibraryId};
use scorarium_engine::person::{Person, PersonId, derive_sort_name};
use serde::Deserialize;

use crate::auth::{Session, Viewer};
use crate::error::Result;
use crate::page::library::{entity, view};
use crate::page::{Base, Crumb, form_error};
use crate::widget::header::Header;

#[derive(Deserialize)]
pub(crate) struct Form {
    name: String,
    sort_name: String,
}

#[derive(Template)]
#[template(path = "page/library/person_edit.html")]
struct Edit {
    base: Base,
    library: LibraryId,
    id: PersonId,
    draft: bool,
    name: String,
    sort_name: String,
    error: Option<String>,
}

fn render(
    header: Header,
    library: &Library,
    person: &Person,
    form: Form,
    status: StatusCode,
    error: Option<String>,
) -> Result<Response> {
    let parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), format!("/library/{}", library.id())),
    ];
    let page = Edit {
        base: Base::new(header, person.name(), parents),
        library: library.id(),
        id: person.id(),
        draft: person.is_draft(),
        name: form.name,
        sort_name: form.sort_name,
        error,
    };
    Ok((status, Html(page.render()?)).into_response())
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
    Path((library, person)): Path<(String, String)>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let person: Person = entity::load(&engine, library.id(), &person, &Viewer::LoggedIn).await?;
    let form = Form {
        name: person.name().to_owned(),
        sort_name: person.sort_name().to_owned(),
    };
    render(header, &library, &person, form, StatusCode::OK, None)
}

pub async fn post(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
    Path((library, person)): Path<(String, String)>,
    axum::Form(form): axum::Form<Form>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let mut person: Person =
        entity::load(&engine, library.id(), &person, &Viewer::LoggedIn).await?;
    let sort_name = match form.sort_name.trim() {
        "" => derive_sort_name(&form.name),
        sort_name => sort_name.to_owned(),
    };
    match person.rename(&form.name, &sort_name).await {
        Ok(()) => Ok(
            Redirect::to(&format!("/library/{}/person/{}", library.id(), person.id()))
                .into_response(),
        ),
        Err(e) => {
            let (status, error) = form_error(e)?;
            render(header, &library, &person, form, status, Some(error))
        }
    }
}
