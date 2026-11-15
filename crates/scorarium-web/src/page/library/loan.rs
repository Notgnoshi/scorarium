use axum::extract::{Path, Query, State};
use axum::response::Redirect;
use scorarium_engine::Engine;
use scorarium_engine::entity::Entity;
use scorarium_engine::holding::HoldingId;
use scorarium_engine::publication::Publication;
use scorarium_engine::value::NonBlank;
use serde::Deserialize;

use crate::auth::{Session, Viewer};
use crate::error::Result;
use crate::page::library::{entity, view};
use crate::redirect::{Back, BackQuery};

#[derive(Deserialize)]
pub(crate) struct LendForm {
    borrower: String,
}

/// Lend a physical copy to the borrower in the form
pub async fn lend(
    _session: Session,
    State(engine): State<Engine>,
    Path((library, publication, holding)): Path<(String, String, String)>,
    Query(query): Query<BackQuery>,
    axum::Form(form): axum::Form<LendForm>,
) -> Result<Redirect> {
    let library = view::load(&engine, &library).await?;
    let mut publication: Publication =
        entity::load(&engine, library.id(), &publication, &Viewer::LoggedIn).await?;
    let holding: HoldingId = holding
        .parse()
        .map_err(|_| scorarium_engine::Error::NotFound)?;
    // A blank name would mark the copy returned.
    let borrower: NonBlank = form.borrower.parse()?;
    publication
        .set_holding_loaned_to(holding, Some(borrower.as_str()))
        .await?;
    let back = Back::new(
        query,
        format!("/library/{}/publication/{}", library.id(), publication.id()),
    );
    Ok(Redirect::to(back.path()))
}
