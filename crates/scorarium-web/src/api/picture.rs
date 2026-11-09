use axum::Extension;
use axum::extract::{Multipart, Path, State};
use axum::response::{Html, IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::publication::Publication;

use crate::Settings;
use crate::api::upload;
use crate::auth::{ApiSession, Viewer};
use crate::error::Result;
use crate::page::library::{entity, view};
use crate::widget::picture_box::PictureBox;

/// Replace a publication's cover with the uploaded file, and answer with the picture box
pub async fn upload_cover(
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
    publication.set_cover(source).await?;
    let picture_box = PictureBox::cover(&publication, settings.files_allowed(&engine));
    Ok(Html(askama::Template::render(&picture_box)?).into_response())
}

/// Remove a publication's cover, and answer with the picture box
pub async fn remove_cover(
    _session: ApiSession,
    State(engine): State<Engine>,
    Extension(settings): Extension<Settings>,
    Path((library, publication)): Path<(String, String)>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let mut publication: Publication =
        entity::load(&engine, library.id(), &publication, &Viewer::LoggedIn).await?;
    publication.clear_cover().await?;
    let picture_box = PictureBox::cover(&publication, settings.files_allowed(&engine));
    Ok(Html(askama::Template::render(&picture_box)?).into_response())
}
