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
    Path(path): Path<(String, String, String)>,
    Query(query): Query<BackQuery>,
    axum::Form(form): axum::Form<LendForm>,
) -> Result<Redirect> {
    // A blank name would mark the copy returned.
    let borrower: NonBlank = form.borrower.parse()?;
    set_borrower(&engine, path, query, Some(borrower.as_str())).await
}

/// Mark a lent copy as returned
pub async fn mark_returned(
    _session: Session,
    State(engine): State<Engine>,
    Path(path): Path<(String, String, String)>,
    Query(query): Query<BackQuery>,
) -> Result<Redirect> {
    set_borrower(&engine, path, query, None).await
}

/// Set or clear the borrower of a holding, and go back to the page that asked
async fn set_borrower(
    engine: &Engine,
    (library, publication, holding): (String, String, String),
    query: BackQuery,
    borrower: Option<&str>,
) -> Result<Redirect> {
    let library = view::load(engine, &library).await?;
    let mut publication: Publication =
        entity::load(engine, library.id(), &publication, &Viewer::LoggedIn).await?;
    let holding: HoldingId = holding
        .parse()
        .map_err(|_| scorarium_engine::Error::NotFound)?;
    publication.set_holding_loaned_to(holding, borrower).await?;
    let back = Back::new(
        query,
        format!("/library/{}/publication/{}", library.id(), publication.id()),
    );
    Ok(Redirect::to(back.path()))
}
