use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::holding::{HoldingId, NewHolding};
use scorarium_engine::publication::Publication;
use serde::Deserialize;

use crate::auth::{ApiSession, Viewer};
use crate::error::Result;
use crate::page::library::{entity, view};
use crate::widget::holdings::{HoldingAdd, HoldingEntry};

#[derive(Deserialize)]
pub(crate) struct LocationForm {
    holding: String,
    location: String,
}

/// Move a physical holding, and answer with its entry
pub async fn location(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((library, publication)): Path<(String, String)>,
    axum::Form(form): axum::Form<LocationForm>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let mut publication: Publication =
        entity::load(&engine, library.id(), &publication, &Viewer::LoggedIn).await?;
    let id: HoldingId = form
        .holding
        .parse()
        .map_err(|_| scorarium_engine::Error::NotFound)?;
    publication
        .set_physical_holding_location(id, Some(&form.location))
        .await?;
    let holding = publication
        .holdings()
        .iter()
        .find(|holding| holding.id == id)
        .ok_or(scorarium_engine::Error::NotFound)?;
    let entry = HoldingEntry::new(&publication, holding);
    Ok(Html(askama::Template::render(&entry)?).into_response())
}

#[derive(Deserialize)]
pub(crate) struct AddForm {
    location: String,
}

/// Add a physical holding, and answer with its entry
pub async fn add(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((library, publication)): Path<(String, String)>,
    axum::Form(form): axum::Form<AddForm>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let mut publication: Publication =
        entity::load(&engine, library.id(), &publication, &Viewer::LoggedIn).await?;
    let id = publication
        .add_holding(NewHolding::Physical {
            location: Some(&form.location),
        })
        .await?;
    let holding = publication
        .holdings()
        .iter()
        .find(|holding| holding.id == id)
        .ok_or(scorarium_engine::Error::NotFound)?;
    let entry = HoldingEntry::new(&publication, holding);
    // The fresh form replaces the used one out of band, which collapses and clears it.
    let add = HoldingAdd::new(&publication, true);
    Ok(Html(format!(
        "{}{}",
        askama::Template::render(&entry)?,
        askama::Template::render(&add)?
    ))
    .into_response())
}
