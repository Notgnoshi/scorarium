use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use eyre::WrapErr;
use http::{HeaderMap, StatusCode};
use url::Url;

use crate::log::CallLog;
use crate::response_file;
use crate::transport::request_key;

const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const NOT_FOUND_MAX_AGE: Duration = Duration::from_secs(2 * 24 * 60 * 60);

/// Caches raw API responses as files in a directory.
///
/// The index is an authoritative reference of what cached responses are on disk to avoid spamming stat()s
pub(crate) struct DiskCache {
    dir: PathBuf,
    index: Mutex<HashMap<PathBuf, SystemTime>>,
    log: Arc<CallLog>,
}

impl DiskCache {
    /// Open the cache in `dir`, creating the directory if it is missing.
    ///
    /// Delete every cached response older than 30 days and index the rest. Leave every file that
    /// the cache did not write alone.
    pub(crate) fn open(dir: PathBuf, log: Arc<CallLog>) -> eyre::Result<DiskCache> {
        std::fs::create_dir_all(&dir)
            .wrap_err_with(|| format!("Failed to create {}", dir.display()))?;
        tempfile::NamedTempFile::new_in(&dir)
            .wrap_err_with(|| format!("{} is not writable", dir.display()))?;

        let mut index = HashMap::new();
        for host in read_dir(&dir)? {
            let host = host.wrap_err_with(|| format!("Failed to read {}", dir.display()))?;
            let host_path = host.path();
            let file_type = host
                .file_type()
                .wrap_err_with(|| format!("Failed to stat {}", host_path.display()))?;
            if !file_type.is_dir() {
                continue;
            }
            for entry in read_dir(&host_path)? {
                let entry =
                    entry.wrap_err_with(|| format!("Failed to read {}", host_path.display()))?;
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                let is_response = name.starts_with("request.");
                if !is_response && !name.starts_with(".request.") {
                    continue;
                }
                // Another process may delete or replace the file after it was listed.
                let metadata = match entry.metadata() {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => {
                        return Err(error)
                            .wrap_err_with(|| format!("Failed to stat {}", path.display()));
                    }
                };
                if !metadata.is_file() {
                    continue;
                }
                let modified = metadata
                    .modified()
                    .wrap_err_with(|| format!("Failed to stat {}", path.display()))?;
                if is_expired(modified, MAX_AGE) {
                    match std::fs::remove_file(&path) {
                        Err(error) if error.kind() != io::ErrorKind::NotFound => {
                            return Err(error)
                                .wrap_err_with(|| format!("Failed to delete {}", path.display()));
                        }
                        _ => {}
                    }
                } else if is_response {
                    index.insert(PathBuf::from(host.file_name()).join(&name), modified);
                }
            }
        }
        Ok(DiskCache {
            dir,
            index: Mutex::new(index),
            log,
        })
    }

    /// Check the index for a response to the request that is younger than 30 days, without
    /// touching the disk.
    pub(crate) fn contains(&self, url: &Url, headers: &HeaderMap) -> bool {
        let Ok((path, _)) = locate(url, headers) else {
            return false;
        };
        let index = self.index.lock().unwrap();
        index
            .get(&path)
            .is_some_and(|&modified| !is_expired(modified, MAX_AGE))
    }

    /// Read the cached response to the request.
    ///
    /// Return `None` and forget the response if it has expired, its file is gone, or its file
    /// holds a different request.
    pub(crate) async fn get(
        &self,
        url: &Url,
        headers: &HeaderMap,
    ) -> Option<http::Response<Bytes>> {
        let (path, key) = locate(url, headers).ok()?;
        let modified = self.index.lock().unwrap().get(&path).copied()?;
        if is_expired(modified, MAX_AGE) {
            self.remove(url, headers).await;
            return None;
        }

        let file = self.dir.join(&path);
        let read = blocking({
            let file = file.clone();
            move || response_file::read(&file)
        })
        .await;
        match read {
            Ok(Some((stored, response))) if stored == key => {
                if response.status() == StatusCode::NOT_FOUND
                    && is_expired(modified, NOT_FOUND_MAX_AGE)
                {
                    self.remove(url, headers).await;
                    return None;
                }
                self.log.cache_hit();
                return Some(response);
            }
            Ok(Some(_)) => tracing::error!("{} holds a different request", file.display()),
            Ok(None) => {}
            Err(error) => tracing::error!("Discarding the cached response: {error:#}"),
        }
        self.index.lock().unwrap().remove(&path);
        None
    }

    /// Cache the response to the request.
    pub(crate) async fn insert(
        &self,
        url: &Url,
        headers: &HeaderMap,
        response: &http::Response<Bytes>,
    ) {
        let (path, key) = match locate(url, headers) {
            Ok(located) => located,
            Err(error) => return tracing::error!("Not caching the response: {error:#}"),
        };
        let file = self.dir.join(&path);
        let response = response.clone();
        match blocking(move || response_file::write(&file, &key, &response)).await {
            Ok(()) => {
                self.index.lock().unwrap().insert(path, SystemTime::now());
            }
            Err(error) => tracing::error!("Not caching the response: {error:#}"),
        }
    }

    /// Forget the cached response to the request and delete its file.
    pub(crate) async fn remove(&self, url: &Url, headers: &HeaderMap) {
        let (path, _) = match locate(url, headers) {
            Ok(located) => located,
            Err(error) => return tracing::error!("Not removing the response: {error:#}"),
        };
        self.index.lock().unwrap().remove(&path);
        let file = self.dir.join(path);
        let removed = blocking({
            let file = file.clone();
            move || match std::fs::remove_file(&file) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.into()),
                _ => Ok(()),
            }
        })
        .await;
        if let Err(error) = removed {
            tracing::error!("Failed to delete {}: {error:#}", file.display());
        }
    }
}

fn locate(url: &Url, headers: &HeaderMap) -> eyre::Result<(PathBuf, String)> {
    let key = request_key(url, headers);
    Ok((response_file::relative_path(url, &key)?, key))
}

/// Check whether `modified` is older than `max_age`, counting a time in the future as fresh.
fn is_expired(modified: SystemTime, max_age: Duration) -> bool {
    SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|age| age > max_age)
}

fn read_dir(dir: &Path) -> eyre::Result<impl Iterator<Item = io::Result<std::fs::DirEntry>>> {
    std::fs::read_dir(dir).wrap_err_with(|| format!("Failed to read {}", dir.display()))
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> eyre::Result<T> + Send + 'static,
) -> eyre::Result<T> {
    tokio::task::spawn_blocking(work).await?
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use http::HeaderValue;
    use http::header::ACCEPT;

    use super::*;

    fn request() -> (Url, HeaderMap) {
        let url = Url::parse("https://openlibrary.org/search.json?title=rondos").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        (url, headers)
    }

    fn not_found() -> http::Response<Bytes> {
        let mut response = http::Response::new(Bytes::from_static(b"{}"));
        *response.status_mut() = StatusCode::NOT_FOUND;
        response
    }

    #[tokio::test]
    async fn a_response_outlives_the_cache_that_stored_it() {
        let dir = tempfile::tempdir().unwrap();
        let (url, headers) = request();
        let first = DiskCache::open(dir.path().to_owned(), Arc::new(CallLog::default())).unwrap();
        first.insert(&url, &headers, &not_found()).await;

        let log = Arc::new(CallLog::default());
        let second = DiskCache::open(dir.path().to_owned(), log.clone()).unwrap();

        assert!(second.contains(&url, &headers));
        let hit = second
            .get(&url, &headers)
            .await
            .expect("cached by the first");
        assert_eq!(hit.status(), StatusCode::NOT_FOUND);
        assert_eq!(hit.body(), &Bytes::from_static(b"{}"));
        assert!(!second.contains(&url, &HeaderMap::new()));
        assert_eq!(log.history().cache_hits, 1);
    }

    #[tokio::test]
    async fn a_response_older_than_30_days_is_deleted_at_startup() {
        let dir = tempfile::tempdir().unwrap();
        let (url, headers) = request();
        let log = Arc::new(CallLog::default());
        let cache = DiskCache::open(dir.path().to_owned(), log.clone()).unwrap();
        cache.insert(&url, &headers, &not_found()).await;
        let key = request_key(&url, &headers);
        let file = dir
            .path()
            .join(response_file::relative_path(&url, &key).unwrap());
        let stale = SystemTime::now() - Duration::from_secs(31 * 24 * 60 * 60);
        File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(stale)
            .unwrap();

        let reopened = DiskCache::open(dir.path().to_owned(), log).unwrap();

        assert!(!reopened.contains(&url, &headers));
        assert!(reopened.get(&url, &headers).await.is_none());
        assert!(!file.exists());
    }
}
