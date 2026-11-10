use axum::body::Body;
use axum::extract::{Multipart, Path, Request, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS};
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use axum::{Extension, Json};
use scorarium_engine::Engine;
use scorarium_engine::asset::StagedId;
use serde::Serialize;
use tower_http::services::ServeFile;

use crate::Settings;
use crate::api::upload;
use crate::auth::ApiSession;
use crate::error::{Error, Result};
use crate::page::library::file::DISPLAYED_TYPES;

#[derive(Serialize)]
pub(crate) struct Staged {
    id: String,
    name: String,
    size: u64,
}

/// Stage the uploaded file or the URL, and return its id, name, and size
pub async fn post(
    _session: ApiSession,
    State(engine): State<Engine>,
    Extension(settings): Extension<Settings>,
    mut form: Multipart,
) -> Result<Json<Staged>> {
    let source = upload::source(&engine, settings, &mut form).await?;
    let staged = engine.stage(source).await?;
    Ok(Json(Staged {
        id: staged.id.to_string(),
        name: staged.filename,
        size: staged.size,
    }))
}

/// Serve a staged file that a browser may display, for a preview
pub async fn get(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path(staged): Path<String>,
    request: Request,
) -> Result<Response> {
    let Ok(id) = staged.parse::<StagedId>();
    let staged = engine
        .staged(&id)
        .ok_or(scorarium_engine::Error::NotFound)?;
    let content_type = infer::get_from_path(&staged.file)
        .map_err(scorarium_engine::Error::from)?
        .map(|kind| kind.mime_type())
        .filter(|mime| DISPLAYED_TYPES.contains(mime))
        .ok_or(scorarium_engine::Error::NotFound)?;

    let response = ServeFile::new(&staged.file)
        .try_call(request)
        .await
        .map_err(|e| Error::new(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if response.status() == StatusCode::NOT_FOUND {
        return Err(scorarium_engine::Error::NotFound.into());
    }
    let mut response = response.map(Body::new);
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-cache"));
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    Ok(response)
}

/// Discard a staged file
pub async fn discard(
    _session: ApiSession,
    State(engine): State<Engine>,
    Path(staged): Path<String>,
) -> StatusCode {
    let Ok(id) = staged.parse::<StagedId>();
    engine.discard_staged(&id);
    StatusCode::NO_CONTENT
}
