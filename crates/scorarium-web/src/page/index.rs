use askama::Template;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::Engine;
use scorarium_engine::library::LibraryId;
use serde::Deserialize;

use crate::auth::{Session, Viewer};
use crate::error::Result;
use crate::page::{Base, form_error};
use crate::widget::header::Header;

#[derive(Template)]
#[template(path = "page/index.html")]
struct Index {
    base: Base,
    logged_in: bool,
    error: Option<String>,
    libraries: Vec<Summary>,
}

struct Summary {
    id: LibraryId,
    name: String,
    private: bool,
    publications: usize,
}

#[derive(Deserialize)]
pub(crate) struct Form {
    name: String,
}

async fn render(
    viewer: &Viewer,
    engine: &Engine,
    header: Header,
    status: StatusCode,
    error: Option<String>,
) -> Result<Response> {
    let mut libraries = Vec::new();
    for library in engine.libraries().await? {
        if library.is_private() && !viewer.is_logged_in() {
            continue;
        }
        libraries.push(Summary {
            id: library.id(),
            name: library.name().to_owned(),
            private: library.is_private(),
            publications: library.publication_count(false).await?,
        });
    }
    let page = Index {
        base: Base::new(header, "Libraries", vec![]),
        logged_in: viewer.is_logged_in(),
        error,
        libraries,
    };
    Ok((status, Html(page.render()?)).into_response())
}

pub async fn get(viewer: Viewer, State(engine): State<Engine>, header: Header) -> Result<Response> {
    render(&viewer, &engine, header, StatusCode::OK, None).await
}

pub async fn post(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
    axum::Form(form): axum::Form<Form>,
) -> Result<Response> {
    match engine.create_library(&form.name, true).await {
        Ok(library) => Ok(Redirect::to(&format!("/library/{}", library.id())).into_response()),
        Err(e) => {
            let (status, error) = form_error(e)?;
            render(&Viewer::LoggedIn, &engine, header, status, Some(error)).await
        }
    }
}
