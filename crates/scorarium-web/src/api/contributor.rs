use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::contributor::Role;
use scorarium_engine::entity::{Contributed, Entity};
use scorarium_engine::person::Person;
use serde::Deserialize;

use crate::auth::{ApiSession, Viewer};
use crate::error::Result;
use crate::page::form_error;
use crate::page::library::{entity, view};
use crate::widget::contributors::Contributors;

#[derive(Deserialize)]
pub(crate) struct AddForm {
    name: String,
    /// The picked person's id, or `new` to create a person named `name`
    person: String,
    role: String,
}

#[derive(Deserialize)]
pub(crate) struct RemoveForm {
    person: String,
    role: String,
}

fn invalid(message: &str) -> scorarium_engine::Error {
    scorarium_engine::Error::InvalidValue(message.to_owned())
}

async fn apply_add<E: Contributed>(
    engine: &Engine,
    entity: &mut E,
    form: &AddForm,
) -> scorarium_engine::Result<()> {
    let role: Role = form.role.parse()?;
    match form.person.as_str() {
        "" => Err(invalid("Pick a person from the list.")),
        "new" => entity.create_contributor(&form.name, role).await.map(drop),
        id => {
            let person: Person = entity::in_library(engine, entity.library(), id, "person").await?;
            entity.add_contributor(person.id(), role).await
        }
    }
}

async fn apply_remove<E: Contributed>(
    engine: &Engine,
    entity: &mut E,
    form: &RemoveForm,
) -> scorarium_engine::Result<()> {
    let role: Role = form.role.parse()?;
    let person: Person =
        entity::in_library(engine, entity.library(), &form.person, "person").await?;
    entity.remove_contributor(person.id(), &role).await
}

/// Render the section with the outcome of an action
async fn render<E: Contributed>(
    mut entity: E,
    outcome: scorarium_engine::Result<()>,
) -> Result<Response> {
    let (status, error) = match outcome {
        Ok(()) => (StatusCode::OK, None),
        Err(e) => {
            let (status, error) = form_error(e)?;
            (status, Some(error))
        }
    };
    // Reloading picks up contributors changed from another tab as well as this action's.
    entity.reload().await?;
    let section = Contributors::new(&entity, error);
    Ok((status, Html(askama::Template::render(&section)?)).into_response())
}

pub async fn add<E: Contributed>(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((library, id)): Path<(String, String)>,
    axum::Form(form): axum::Form<AddForm>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let mut entity: E = entity::load(&engine, library.id(), &id, &Viewer::LoggedIn).await?;
    let outcome = apply_add(&engine, &mut entity, &form).await;
    render(entity, outcome).await
}

pub async fn remove<E: Contributed>(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((library, id)): Path<(String, String)>,
    axum::Form(form): axum::Form<RemoveForm>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let mut entity: E = entity::load(&engine, library.id(), &id, &Viewer::LoggedIn).await?;
    let outcome = apply_remove(&engine, &mut entity, &form).await;
    render(entity, outcome).await
}
