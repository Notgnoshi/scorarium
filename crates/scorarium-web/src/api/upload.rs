use axum::extract::Multipart;
use futures_util::TryStreamExt;
use scorarium_engine::asset::AssetSource;
use scorarium_engine::{Engine, Error};
use tokio_util::io::StreamReader;

use crate::Settings;

/// Read the file or the URL that a multipart form submitted
///
/// Returns [Error::InvalidValue] if files are disabled for this demo, if the URL is not an HTTP
/// URL, or if the form does not start with a file or a URL.
pub(crate) async fn source<'a>(
    engine: &Engine,
    settings: Settings,
    form: &'a mut Multipart,
) -> scorarium_engine::Result<AssetSource<'a>> {
    if !settings.files_allowed(engine) {
        return Err(Error::InvalidValue("File uploads are disabled.".to_owned()));
    }
    // Only the first part is read, because a part that borrows the form cannot be returned from a
    // loop over the form's parts.
    let field = form
        .next_field()
        .await
        .map_err(|e| Error::InvalidValue(e.body_text()))?;
    match field {
        Some(field) if field.name() == Some("file") => {
            let filename = field.file_name().unwrap_or_default().to_owned();
            let data = StreamReader::new(field.map_err(std::io::Error::other));
            Ok(AssetSource::Upload {
                data: Box::new(data),
                filename,
            })
        }
        Some(field) if field.name() == Some("url") => {
            let text = field
                .text()
                .await
                .map_err(|e| Error::InvalidValue(e.body_text()))?;
            Ok(AssetSource::Url(text.parse()?))
        }
        _ => Err(Error::InvalidValue(
            "Choose a file or enter a URL.".to_owned(),
        )),
    }
}
