use axum::extract::{Path, State};
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::holding::HoldingId;
use scorarium_engine::publication::Publication;
use serde::Deserialize;

use crate::auth::{ApiSession, Viewer};
use crate::error::Result;
use crate::page::library::{entity, view};
use crate::widget::holdings::HoldingEntry;

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
    let entry = HoldingEntry::new(&publication, holding).saved();
    Ok(Html(askama::Template::render(&entry)?).into_response())
}
