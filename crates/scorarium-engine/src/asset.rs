use std::io::ErrorKind;
use std::path::Path;

use tempfile::{NamedTempFile, TempPath};
use tokio::io::{AsyncRead, AsyncWriteExt};

use crate::audit::AuditSubject;
use crate::{Engine, Error, Result};

/// The origin of a file to add to the library
pub enum AssetSource<'a> {
    /// A file the user uploaded, along with the filename they uploaded it under
    Upload {
        data: Box<dyn AsyncRead + Send + Unpin + 'a>,
        filename: String,
    },
}

/// A file in the staging directory, deleted on drop unless persisted
pub(crate) struct Staged {
    path: TempPath,
}

/// Write the file from `source` into the staging directory
pub(crate) async fn stage(engine: &Engine, source: AssetSource<'_>) -> Result<Staged> {
    let AssetSource::Upload { mut data, .. } = source;
    let (file, path) = NamedTempFile::new_in(engine.staging_dir())?.into_parts();
    let mut file = tokio::fs::File::from_std(file);
    tokio::io::copy(&mut data, &mut file).await?;
    file.flush().await?;
    Ok(Staged { path })
}

impl Staged {
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
