use askama::Template;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::Engine;
use scorarium_engine::library::{Library, LibraryId};
use serde::Deserialize;

use super::view::load;
use crate::auth::Session;
use crate::error::Result;
use crate::page::{Base, Crumb, form_error};
use crate::widget::confirm::{Confirm, count};
use crate::widget::header::Header;

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum Visibility {
    Public,
    Private,
}

#[derive(Deserialize)]
pub(crate) struct Form {
    name: String,
    visibility: Visibility,
}

#[derive(Template)]
#[template(path = "page/library/edit.html")]
struct Edit {
    base: Base,
    id: LibraryId,
    name: String,
    private: bool,
    delete: Confirm,
    error: Option<String>,
}

fn render(
    header: Header,
    library: &Library,
    publications: usize,
    status: StatusCode,
    error: Option<String>,
) -> Result<Response> {
    let page = Edit {
        base: Base::new(
            header,
            "Edit library",
            vec![
                Crumb::new("Libraries", "/"),
                Crumb::new(library.name(), format!("/library/{}", library.id())),
            ],
        ),
        id: library.id(),
        name: library.name().to_owned(),
        private: library.is_private(),
        delete: Confirm {
            id: "delete".to_owned(),
            title: format!("Delete {}?", library.name()),
            message: format!("Deleting {} also deletes:", library.name()),
            also_deleted: vec![
                format!(
                    "its {} and associated assets",
                    count(publications, "publication")
                ),
                "all works and persons".to_owned(),
            ],
            action: "Delete library",
            url: format!("/library/{}/delete", library.id()),
            target: None,
            fields: vec![],
        },
        error,
    };
    Ok((status, Html(page.render()?)).into_response())
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
    Path(id): Path<String>,
) -> Result<Response> {
    let library = load(&engine, &id).await?;
    let publications = library.publication_count(true).await?;
    render(header, &library, publications, StatusCode::OK, None)
}

pub async fn post(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
    Path(id): Path<String>,
    axum::Form(form): axum::Form<Form>,
) -> Result<Response> {
    let mut library = load(&engine, &id).await?;
    let result = match form.name.parse() {
        Ok(name) => library.rename(name).await,
        Err(e) => Err(e),
    };
    let result = match result {
        Ok(()) => {
            library
                .set_private(form.visibility == Visibility::Private)
                .await
        }
        Err(e) => Err(e),
    };
    match result {
        Ok(()) => Ok(Redirect::to(&format!("/library/{}", library.id())).into_response()),
        Err(e) => {
            let (status, error) = form_error(e)?;
            let publications = library.publication_count(true).await?;
            render(header, &library, publications, status, Some(error))
        }
    }
}

pub async fn delete(
    _session: Session,
    State(engine): State<Engine>,
    Path(id): Path<String>,
) -> Result<Redirect> {
    load(&engine, &id).await?.delete().await?;
    Ok(Redirect::to("/"))
}
