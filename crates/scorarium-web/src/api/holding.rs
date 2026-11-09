use axum::Extension;
use axum::extract::{Multipart, Path, State};
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::holding::{Holding, HoldingId, HoldingKind, NewHolding};
use scorarium_engine::publication::Publication;
use serde::Deserialize;

use crate::Settings;
use crate::api::upload;
use crate::auth::{ApiSession, Viewer};
use crate::error::Result;
use crate::page::library::{entity, view};
use crate::widget::confirm::Confirm;
use crate::widget::holdings::{HoldingAddPhysical, HoldingEntry};

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
    // The fresh form replaces the used one out of band, which clears it. Only this form is
    // replaced, because replacing the Digital side would detach the file input of a running
    // transfer.
    let add = HoldingAddPhysical::replacement(&publication);
    Ok(Html(format!(
        "{}{}",
        askama::Template::render(&entry)?,
        askama::Template::render(&add)?
    ))
    .into_response())
}

/// Add a digital holding from the uploaded file or the URL, and answer with its entry
pub async fn upload(
    _session: ApiSession,
    State(engine): State<Engine>,
    Extension(settings): Extension<Settings>,
    Path((library, publication)): Path<(String, String)>,
    mut form: Multipart,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let mut publication: Publication =
        entity::load(&engine, library.id(), &publication, &Viewer::LoggedIn).await?;
    let source = upload::source(&engine, settings, &mut form).await?;
    let id = publication.add_holding(NewHolding::Digital(source)).await?;
    let holding = publication
        .holdings()
        .iter()
        .find(|holding| holding.id == id)
        .ok_or(scorarium_engine::Error::NotFound)?;
    let entry = HoldingEntry::new(&publication, holding);
    Ok(Html(askama::Template::render(&entry)?).into_response())
}

#[derive(Deserialize)]
pub(crate) struct RemoveForm {
    holding: String,
    /// What the user confirmed, which is absent on the first click
    confirmed: Option<Confirmed>,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Confirmed {
    Holding,
    Publication,
}

/// Ask before removing a holding, then remove it, or delete the publication with its last holding
///
/// The dialog depends on how many holdings remain, which another tab can change, so the server
/// chooses it. A confirmation acts only if it still matches the current state.
pub async fn remove(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((library, publication)): Path<(String, String)>,
    axum::Form(form): axum::Form<RemoveForm>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let mut publication: Publication =
        entity::load(&engine, library.id(), &publication, &Viewer::LoggedIn).await?;
    let id: HoldingId = form
        .holding
        .parse()
        .map_err(|_| scorarium_engine::Error::NotFound)?;
    let holding = publication
        .holdings()
        .iter()
        .find(|holding| holding.id == id)
        .ok_or(scorarium_engine::Error::NotFound)?;
    let current = if publication.holdings().len() == 1 {
        Confirmed::Publication
    } else {
        Confirmed::Holding
    };

    if form.confirmed.as_ref() != Some(&current) {
        let dialog = dialog(&publication, holding, &current);
        return Ok(Html(askama::Template::render(&dialog)?).into_response());
    }
    match current {
        Confirmed::Holding => {
            publication.remove_holding(id).await?;
            Ok((
                [
                    ("HX-Retarget", format!("#holding-{id}")),
                    ("HX-Reswap", "delete".to_owned()),
                ],
                "",
            )
                .into_response())
        }
        Confirmed::Publication => {
            publication.delete().await?;
            Ok(([("HX-Redirect", format!("/library/{}", library.id()))], "").into_response())
        }
    }
}

/// Build the dialog that asks the user to confirm removing the holding in the current state
fn dialog(publication: &Publication, holding: &Holding, state: &Confirmed) -> Confirm {
    let title = publication.title().unwrap_or("Untitled");
    let (heading, message, also_deleted, action) = match (state, &holding.kind) {
        (Confirmed::Publication, _) => (
            "Delete this publication?".to_owned(),
            format!(
                "This is the last copy of {title}. Deleting it deletes the publication, along with:"
            ),
            vec![
                "its cover and files".to_owned(),
                "works in no other publication".to_owned(),
                "people who contribute to nothing else".to_owned(),
            ],
            "Delete publication",
        ),
        (Confirmed::Holding, HoldingKind::Physical { location }) => (
            "Delete this copy?".to_owned(),
            match location.as_deref().filter(|location| !location.is_empty()) {
                Some(location) => format!("The copy at {location} is removed from {title}."),
                None => format!("The copy is removed from {title}."),
            },
            vec![],
            "Delete copy",
        ),
        (Confirmed::Holding, HoldingKind::Digital { file }) => (
            "Delete this file?".to_owned(),
            format!(
                "{} is deleted from the library. This cannot be undone.",
                file.file_name()
                    .unwrap_or(file.as_os_str())
                    .to_string_lossy()
            ),
            vec![],
            "Delete file",
        ),
    };
    let confirmed = match state {
        Confirmed::Holding => "holding",
        Confirmed::Publication => "publication",
    };
    Confirm {
        id: format!("holding-{}-confirm", holding.id),
        title: heading,
        message,
        also_deleted,
        action,
        url: format!(
            "/api/library/{}/publication/{}/holding/remove",
            publication.library(),
            publication.id()
        ),
        target: Some(format!("#holding-{}-dialog", holding.id)),
        fields: vec![
            ("holding", holding.id.to_string()),
            ("confirmed", confirmed.to_owned()),
        ],
    }
}
