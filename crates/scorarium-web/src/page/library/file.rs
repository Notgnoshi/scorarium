use std::path;

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::header::{CACHE_CONTROL, X_CONTENT_TYPE_OPTIONS};
use axum::http::{HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use scorarium_engine::Engine;
use scorarium_engine::person::Person;
use scorarium_engine::publication::Publication;
use tower_http::services::ServeFile;

use crate::auth::{Viewer, login_redirect};
use crate::error::{Error, Result};
use crate::page::library::{entity, view};

/// Serve a publication's cover image
pub async fn cover(
    viewer: Viewer,
    method: Method,
    uri: Uri,
    State(engine): State<Engine>,
    Path((library, publication)): Path<(String, String)>,
    request: Request,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    if library.is_private() && !viewer.is_logged_in() {
        return Ok(login_redirect(&method, &uri).into_response());
    }
    let publication: Publication =
        entity::load(&engine, library.id(), &publication, &viewer).await?;
    let path = publication
        .cover()
        .ok_or(scorarium_engine::Error::NotFound)?;
    picture(path, request).await
}

/// Serve a person's profile picture
pub async fn profile(
    viewer: Viewer,
    method: Method,
    uri: Uri,
    State(engine): State<Engine>,
    Path((library, person)): Path<(String, String)>,
    request: Request,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    if library.is_private() && !viewer.is_logged_in() {
        return Ok(login_redirect(&method, &uri).into_response());
    }
    let person: Person = entity::load(&engine, library.id(), &person, &viewer).await?;
    let path = person.profile().ok_or(scorarium_engine::Error::NotFound)?;
    picture(path, request).await
}

/// Serve the picture at `path`, cached for a year by the browser and never by a shared cache
///
/// The URL carries a `v` query parameter that changes with the picture, so the cached copy is
/// immutable.
async fn picture(path: &path::Path, request: Request) -> Result<Response> {
    let response = ServeFile::new(path)
        .try_call(request)
        .await
        .map_err(|e| Error::new(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let mut response = response.map(Body::new);
    let headers = response.headers_mut();
    headers.insert(
        CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=31536000, immutable"),
    );
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    Ok(response)
}
