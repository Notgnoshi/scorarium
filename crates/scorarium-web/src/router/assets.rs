//! Files under assets/ embedded in the binary and served under /assets/.
//!
//! Asset URLs carry a hash of the file's content, so browsers can cache them forever and still
//! pick up a new version the moment a page links to it. In debug builds rust-embed reads the
//! files from disk on every request instead of embedding them, so an edited stylesheet shows up
//! on reload without a rebuild; that is also why the hash is not cached here.

use axum::extract::Path;
use axum::http::StatusCode;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "assets/"]
struct Assets;

/// The cache-busting URL of an asset. An unknown name is a programming error that every page test
/// would trip on.
pub fn url(name: &str) -> String {
    let file = Assets::get(name).unwrap_or_else(|| panic!("unknown asset {name}"));
    let hash = file.metadata.sha256_hash();
    // Half the hash is more than enough to tell versions apart and keeps URLs readable
    let hex: String = hash[..8].iter().map(|b| format!("{b:02x}")).collect();
    format!("/assets/{name}?v={hex}")
}

pub async fn asset(Path(name): Path<String>) -> Response {
    // rust-embed refuses paths that escape the folder, which matters in debug builds where it
    // reads from disk
    let Some(file) = Assets::get(&name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // The URL changes whenever the content does, so the browser may keep this forever. The Cow
    // goes straight into the body: borrowed embedded bytes are served without a copy.
    (
        [
            (CONTENT_TYPE, file.metadata.mimetype()),
            (CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        file.data,
    )
        .into_response()
}
