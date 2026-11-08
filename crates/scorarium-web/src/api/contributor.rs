use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::contributor::Role;
use scorarium_engine::entity::{Contributed, Entity};
use scorarium_engine::library::LibraryId;
use scorarium_engine::person::{Person, PersonId};
use scorarium_engine::work::Work;
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

/// Parse a person id that must belong to `library`
async fn person_in(
    engine: &Engine,
    library: LibraryId,
    id: &str,
) -> scorarium_engine::Result<PersonId> {
    let not_here = || invalid("That person is not in this library.");
    let id: PersonId = id.parse().map_err(|_| not_here())?;
    match Person::load(engine, id).await? {
        Some(person) if person.library() == library => Ok(id),
        _ => Err(not_here()),
    }
}

async fn apply_add(
    engine: &Engine,
    work: &mut Work,
    form: &AddForm,
) -> scorarium_engine::Result<()> {
    let role: Role = form.role.parse()?;
    match form.person.as_str() {
        "" => Err(invalid("Pick a person from the list.")),
        "new" => work.create_contributor(&form.name, role).await.map(drop),
        id => {
            let person = person_in(engine, work.library(), id).await?;
            work.add_contributor(person, role).await
        }
    }
}

async fn apply_remove(
    engine: &Engine,
    work: &mut Work,
    form: &RemoveForm,
) -> scorarium_engine::Result<()> {
    let role: Role = form.role.parse()?;
    let person = person_in(engine, work.library(), &form.person).await?;
    work.remove_contributor(person, &role).await
}

/// Render the section with the outcome of an action
async fn render(mut work: Work, outcome: scorarium_engine::Result<()>) -> Result<Response> {
    let (status, error) = match outcome {
        Ok(()) => (StatusCode::OK, None),
        Err(e) => {
            let (status, error) = form_error(e)?;
            (status, Some(error))
        }
    };
    // Reloading picks up contributors changed from another tab as well as this action's.
    work.reload().await?;
    let section = Contributors::new(&work, error);
    Ok((status, Html(askama::Template::render(&section)?)).into_response())
}

pub async fn add(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((library, work)): Path<(String, String)>,
    axum::Form(form): axum::Form<AddForm>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let mut work: Work = entity::load(&engine, library.id(), &work, &Viewer::LoggedIn).await?;
    let outcome = apply_add(&engine, &mut work, &form).await;
    render(work, outcome).await
}

pub async fn remove(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((library, work)): Path<(String, String)>,
    axum::Form(form): axum::Form<RemoveForm>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let mut work: Work = entity::load(&engine, library.id(), &work, &Viewer::LoggedIn).await?;
    let outcome = apply_remove(&engine, &mut work, &form).await;
    render(work, outcome).await
}
