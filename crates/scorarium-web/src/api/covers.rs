use axum::extract::{Path, State};
use axum::http::StatusCode;
use scorarium_engine::Engine;
use scorarium_engine::publication::Publication;

use crate::auth::{ApiSession, Viewer};
use crate::error::Result;
use crate::page::library::{entity, view};

/// Search for more covers of a publication
pub async fn search(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path((library, id)): Path<(String, String)>,
) -> Result<StatusCode> {
    let library = view::load(&engine, &library).await?;
    let publication: Publication =
        entity::load(&engine, library.id(), &id, &Viewer::LoggedIn).await?;
    publication.search_covers();
    Ok(StatusCode::NO_CONTENT)
}
