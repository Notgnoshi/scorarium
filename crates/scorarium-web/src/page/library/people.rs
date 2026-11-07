use std::str::FromStr;

use askama::Template;
use axum::extract::{Path, State};
use axum::http::{Method, Uri};
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::contributor::Role;
use scorarium_engine::entity::EntityKind;
use scorarium_engine::library::LibraryId;
use scorarium_engine::person::PersonSummary;
use scorarium_engine::value::Classification;

use crate::auth::{Viewer, login_redirect};
use crate::error::Result;
use crate::page::library::entity::plural_role_label;
use crate::page::library::view;
use crate::page::{Base, Crumb};
use crate::widget::header::Header;
use crate::widget::library_nav::{Active, LibraryNav};

#[derive(Template)]
#[template(path = "page/library/people.html")]
struct People {
    base: Base,
    library: LibraryId,
    nav: LibraryNav,
    people: Vec<PersonSummary>,
}

pub async fn get(
    viewer: Viewer,
    method: Method,
    uri: Uri,
    State(engine): State<Engine>,
    header: Header,
    Path(library): Path<String>,
) -> Result<Response> {
    render(viewer, method, uri, engine, header, &library, None).await
}

pub async fn get_role(
    viewer: Viewer,
    method: Method,
    uri: Uri,
    State(engine): State<Engine>,
    header: Header,
    Path((library, role)): Path<(String, String)>,
) -> Result<Response> {
    render(viewer, method, uri, engine, header, &library, Some(&role)).await
}

async fn render(
    viewer: Viewer,
    method: Method,
    uri: Uri,
    engine: Engine,
    header: Header,
    library: &str,
    role: Option<&str>,
) -> Result<Response> {
    let library = view::load(&engine, library).await?;
    if library.is_private() && !viewer.is_logged_in() {
        return Ok(login_redirect(&method, &uri).into_response());
    }
    let role = role
        .map(Role::from_str)
        .transpose()
        .map_err(|_| scorarium_engine::Error::NotFound)?;
    let people = library.people_summary(role.as_ref(), false).await?;

    let library_href = format!("/library/{}", library.id());
    let mut parents = vec![
        Crumb::new("Libraries", "/"),
        Crumb::new(library.name(), &library_href),
    ];
    let heading = match &role {
        Some(role) => {
            parents.push(Crumb::new("People", format!("{library_href}/people")));
            plural_role_label(role)
        }
        None => "People".to_owned(),
    };
    let active = match &role {
        Some(role) => Active::Role(role.clone()),
        None => Active::People,
    };
    let page = People {
        base: Base::new(header, heading, parents),
        library: library.id(),
        nav: LibraryNav::load(&library, viewer.is_logged_in(), active).await?,
        people,
    };
    Ok(Html(page.render()?).into_response())
}
