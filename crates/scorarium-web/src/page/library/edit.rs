use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use scorarium_engine::Engine;
use scorarium_engine::library::{Library, LibraryId};
use serde::Deserialize;

use super::view::load;
use crate::auth::Session;
use crate::error::Result;
use crate::page::{Base, Crumb, form_error};
use crate::redirect::{Back, BackQuery};
use crate::widget::confirm::{Confirm, count};
use crate::widget::edit_actions::{EditActions, EditMode};
use crate::widget::edit_heading::EditHeading;
use crate::widget::header::Header;
use crate::widget::refusals::Refusals;

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
    heading: EditHeading,
    actions: EditActions,
    back: Back,
    refusals: Refusals,
}

fn render(
    header: Header,
    library: &Library,
    publications: usize,
    (name, private): (&str, bool),
    back: Back,
    refusals: Refusals,
) -> Result<Response> {
    let status = if refusals.is_empty() {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
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
        name: name.to_owned(),
        private,
        heading: EditHeading {
            title: library.name().to_owned(),
            draft: false,
        },
        actions: EditActions {
            mode: EditMode::Committed,
            cancel: back.path().to_owned(),
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
        },
        back,
        refusals,
    };
    Ok((status, Html(page.render()?)).into_response())
}

pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
    Path(id): Path<String>,
    Query(query): Query<BackQuery>,
) -> Result<Response> {
    let library = load(&engine, &id).await?;
    let back = Back::new(query, format!("/library/{}", library.id()));
    let publications = library.publication_count(true).await?;
    render(
        header,
        &library,
        publications,
        (library.name(), library.is_private()),
        back,
        Refusals::default(),
    )
}

pub async fn post(
    _session: Session,
    State(engine): State<Engine>,
    header: Header,
    Path(id): Path<String>,
    Query(query): Query<BackQuery>,
    axum::Form(form): axum::Form<Form>,
) -> Result<Response> {
    let mut library = load(&engine, &id).await?;
    let back = Back::new(query, format!("/library/{}", library.id()));
    let private = form.visibility == Visibility::Private;
    let mut refusals = Refusals::default();
    if let Some(name) = refusals.parse("name", &form.name) {
        let result = match library.rename(name).await {
            Ok(()) => library.set_private(private).await,
            Err(e) => Err(e),
        };
        if let Err(e) = result {
            let (_, message) = form_error(e)?;
            refusals.refuse(message);
        }
    }
    if refusals.is_empty() {
        return Ok(Redirect::to(back.path_after_save()).into_response());
    }
    let publications = library.publication_count(true).await?;
    render(
        header,
        &library,
        publications,
        (&form.name, private),
        back,
        refusals,
    )
}

pub async fn delete(
    _session: Session,
    State(engine): State<Engine>,
    Path(id): Path<String>,
) -> Result<Redirect> {
    load(&engine, &id).await?.delete().await?;
    Ok(Redirect::to("/"))
}
