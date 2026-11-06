use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;

use crate::error::{Error, Result};

#[derive(rust_embed::Embed)]
#[folder = "assets/"]
struct Assets;

/// Build the versioned URL for an embedded asset.
///
/// Panics on a name that is not in the asset set.
pub fn url(name: &str) -> String {
    let file = Assets::get(name).unwrap_or_else(|| panic!("no asset named {name}"));
    // To avoid browsers caching old versions of assets, add the hash of the asset to the URL
    let hash = file.metadata.sha256_hash();
    let version: String = hash.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("/assets/{name}?v={version}")
}

pub async fn get(Path(name): Path<String>) -> Result<impl IntoResponse> {
    let Some(file) = Assets::get(&name) else {
        return Err(Error::new(
            StatusCode::NOT_FOUND,
            eyre::eyre!("no asset named {name}"),
        ));
    };
    let headers = [
        (header::CONTENT_TYPE, file.metadata.mimetype().to_owned()),
        (
            header::CACHE_CONTROL,
            "public, max-age=31536000, immutable".to_owned(),
        ),
    ];
    Ok((headers, file.data))
}
