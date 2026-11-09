use axum::extract::Multipart;
use futures_util::TryStreamExt;
use scorarium_engine::asset::AssetSource;
use scorarium_engine::{Engine, Error};
use tokio_util::io::StreamReader;

use crate::Settings;

/// Read the file that a multipart form submitted
///
/// Returns [Error::InvalidValue] if files are disabled for this demo, or if the form does not start
/// with a file.
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
    if let Some(field) = field
        && field.name() == Some("file")
    {
        let filename = field.file_name().unwrap_or_default().to_owned();
        let data = StreamReader::new(field.map_err(std::io::Error::other));
        return Ok(AssetSource::Upload {
            data: Box::new(data),
            filename,
        });
    }
    Err(Error::InvalidValue("Choose a file.".to_owned()))
}
