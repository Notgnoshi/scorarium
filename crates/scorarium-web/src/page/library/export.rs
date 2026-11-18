use axum::extract::{Path, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use scorarium_cli::ExportLevel;
use scorarium_engine::Engine;
use scorarium_engine::search::normalize;

use crate::auth::Session;
use crate::error::{Error, Result};
use crate::page::library::view;

/// Serve a library as a shell script download
pub async fn get(
    _session: Session,
    State(engine): State<Engine>,
    Path((library, level)): Path<(String, String)>,
) -> Result<Response> {
    let library = view::load(&engine, &library).await?;
    let export_level = match level.as_str() {
        "publications" => ExportLevel::Publications,
        "works" => ExportLevel::Works,
        "full" => ExportLevel::Full,
        _ => return Err(scorarium_engine::Error::NotFound.into()),
    };

    let mut script = Vec::new();
    scorarium_cli::export_script(&library, export_level, &mut script)
        .await
        .map_err(|e| Error::new(StatusCode::INTERNAL_SERVER_ERROR, e))?;

    let name = normalize(library.name()).replace(' ', "-");
    let filename = if name.is_empty() {
        format!("{level}.scr")
    } else {
        format!("{name}-{level}.scr")
    };
    // The normalized name is ASCII, so the filename needs no encoding.
    let disposition = HeaderValue::try_from(format!("attachment; filename=\"{filename}\""))
        .map_err(|e| Error::new(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let headers = [
        (
            CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        ),
        (CONTENT_DISPOSITION, disposition),
    ];
    Ok((headers, script).into_response())
}
