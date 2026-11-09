use std::path;

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::header::{
    CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
};
use axum::http::{HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use scorarium_engine::Engine;
use scorarium_engine::holding::{HoldingId, HoldingKind};
use scorarium_engine::person::Person;
use scorarium_engine::publication::Publication;
use tower_http::services::ServeFile;

use crate::auth::{Session, Viewer, login_redirect};
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

/// Content types a browser may display in the browser
const DISPLAYED_TYPES: [&str; 8] = [
    "application/pdf",
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "image/avif",
    "text/plain",
    "text/markdown",
];

/// The characters of a filename that need no percent-encoding in a `filename*` parameter
const FILENAME: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Serve the file of a digital holding
///
/// A holding keeps the filename it arrived with, so it may be a type a browser executes, such as
/// HTML or SVG. Only a PDF, raster images, and text are displayed, and every other type is a
/// download.
pub async fn holding(
    _session: Session,
    State(engine): State<Engine>,
    Path((library, publication, holding)): Path<(String, String, String)>,
    request: Request,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let publication: Publication =
        entity::load(&engine, library.id(), &publication, &Viewer::LoggedIn).await?;
    let id: HoldingId = holding
        .parse()
        .map_err(|_| scorarium_engine::Error::NotFound)?;
    let file = publication
        .holdings()
        .iter()
        .find_map(|holding| match &holding.kind {
            HoldingKind::Digital { file } if holding.id == id => Some(file),
            _ => None,
        })
        .ok_or(scorarium_engine::Error::NotFound)?;
    let filename = file
        .file_name()
        .ok_or(scorarium_engine::Error::NotFound)?
        .to_string_lossy();

    let response = ServeFile::new(file)
        .try_call(request)
        .await
        .map_err(|e| Error::new(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(scorarium_engine::Error::NotFound.into());
    }
    let mut response = response.map(Body::new);
    let headers = response.headers_mut();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-cache"));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    // A 304 has no content type, and a browser replaces its cached copy's headers with a 304's, so
    // a disposition on it would override the one chosen for the original response.
    if let Some(content_type) = headers.get(CONTENT_TYPE) {
        let disposition = if DISPLAYED_TYPES.iter().any(|t| content_type == *t) {
            "inline"
        } else {
            "attachment"
        };
        let encoded = utf8_percent_encode(&filename, FILENAME);
        let value = HeaderValue::try_from(format!("{disposition}; filename*=UTF-8''{encoded}"))
            .map_err(|e| Error::new(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        headers.insert(CONTENT_DISPOSITION, value);
    }
    Ok(response)
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
