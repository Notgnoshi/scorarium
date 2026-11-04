use std::io::ErrorKind;
use std::path::Path;

use percent_encoding::percent_decode_str;
use reqwest::header::{CONTENT_DISPOSITION, HeaderMap};
use tempfile::{NamedTempFile, TempPath};
use tokio::io::{AsyncRead, AsyncWriteExt};

use crate::audit::AuditSubject;
use crate::link::HttpUrl;
use crate::{Engine, Error, Result};

/// The origin of a file to add to the library
pub enum AssetSource<'a> {
    /// A file the user uploaded, along with the filename they uploaded it under
    Upload {
        data: Box<dyn AsyncRead + Send + Unpin + 'a>,
        filename: String,
    },
    /// A file to download from the given HTTP URL
    Url(HttpUrl),
}

/// A file in the staging directory, deleted on drop unless persisted
pub(crate) struct Staged {
    path: TempPath,
    filename: String,
}

/// Write the file from `source` into the staging directory
pub(crate) async fn stage(engine: &Engine, source: AssetSource<'_>) -> Result<Staged> {
    let (file, path) = NamedTempFile::new_in(engine.staging_dir())?.into_parts();
    let mut file = tokio::fs::File::from_std(file);
    let filename = match source {
        AssetSource::Upload { mut data, filename } => {
            let filename = component(&filename)
                .ok_or_else(|| Error::InvalidValue("A filename cannot be blank.".into()))?;
            tokio::io::copy(&mut data, &mut file).await?;
            Some(filename)
        }
        AssetSource::Url(url) => {
            let mut response = engine
                .downloads()
                .get(url.as_url().clone())
                .send()
                .await
                .map_err(eyre::Report::new)?;
            let status = response.status();
            if !status.is_success() {
                return Err(Error::InvalidValue(format!("{url} answered {status}.")));
            }
            let filename = download_name(&url, response.headers());
            while let Some(chunk) = response.chunk().await.map_err(eyre::Report::new)? {
                file.write_all(&chunk).await?;
            }
            filename
        }
    };
    file.flush().await?;
    let filename = match filename {
        Some(filename) => filename,
        None => match infer::get_from_path(&path)? {
            Some(kind) => format!("download.{}", kind.extension()),
            None => "download".into(),
        },
    };
    Ok(Staged { path, filename })
}

/// Get the filename for a download after the last segment of its URL, or else its `Content-Disposition` header
fn download_name(url: &HttpUrl, headers: &HeaderMap) -> Option<String> {
    let from_url = url
        .as_url()
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .and_then(|segment| component(&percent_decode_str(segment).decode_utf8_lossy()));
    from_url.or_else(|| {
        let disposition = headers.get(CONTENT_DISPOSITION)?.to_str().ok()?;
        let filename = disposition.split(';').find_map(|param| {
            let (name, value) = param.split_once('=')?;
            name.trim()
                .eq_ignore_ascii_case("filename")
                .then_some(value)
        })?;
        component(filename.trim().trim_matches('"'))
    })
}

/// Reduce `name` to its last path component, or `None` if no component remains
fn component(name: &str) -> Option<String> {
    let last = name
        .split(['/', '\\'])
        .rfind(|piece| !piece.is_empty())?
        .trim();
    match last {
        "" | "." | ".." => None,
        _ => Some(last.to_owned()),
    }
}

impl Staged {
    /// Get the file's name, a single path component
    #[expect(dead_code)]
    pub(crate) fn filename(&self) -> &str {
        &self.filename
    }

    /// Sniff the file's image format and return its file extension
    pub(crate) fn image_extension(&self) -> Result<&'static str> {
        match infer::get_from_path(&self.path)? {
            Some(kind)
                if matches!(
                    kind.mime_type(),
                    "image/jpeg" | "image/png" | "image/gif" | "image/webp" | "image/avif"
                ) =>
            {
                Ok(kind.extension())
            }
            _ => Err(Error::InvalidValue("The file is not an image.".into())),
        }
    }

    /// Move the file to `dest`, replacing any file already there
    pub(crate) fn persist(self, dest: &Path) -> Result<()> {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        self.path.persist(dest).map_err(|e| e.error)?;
        Ok(())
    }
}

/// Remove the asset directory of each entity in `deleted`
///
/// Call this only after the transaction that deleted the entities commits.
pub(crate) fn remove_dirs(engine: &Engine, deleted: &[AuditSubject]) {
    for subject in deleted {
        let AuditSubject::Entity { id, .. } = subject else {
            continue;
        };
        let dir = engine
            .data_dir()
            .join("assets")
            .join(id.kind().to_string())
            .join(id.get().to_string());
        match std::fs::remove_dir_all(&dir) {
            Err(e) if e.kind() != ErrorKind::NotFound => {
                // The rows are already gone, so an error would tell the caller the delete failed.
                log::warn!("failed to remove {}: {e}", dir.display());
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_are_single_components() {
        assert_eq!(component("C:\\scores\\a.pdf").as_deref(), Some("a.pdf"));
        assert_eq!(component("../../a.pdf").as_deref(), Some("a.pdf"));
        assert_eq!(component(" score.pdf ").as_deref(), Some("score.pdf"));
        assert_eq!(component("/"), None);
        assert_eq!(component(".."), None);
        assert_eq!(component(""), None);
    }
}
