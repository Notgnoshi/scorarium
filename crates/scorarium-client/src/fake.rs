use std::path::{Path, PathBuf};
use std::time::Duration;

use bytes::Bytes;
use eyre::{WrapErr, eyre};
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use sha2::{Digest, Sha256};
use url::Url;

use crate::UserAgent;
use crate::transport::{BoxFuture, ReqwestTransport, Transport, request_key};

/// If this environment variable is set to anything, make live API requests and cache them.
/// Otherwise serve the cached responses.
const RECORD: &str = "SCORARIUM_TEST_RECORD_LIVE_API";

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures");

/// Serves cached API responses from the repository's `fixtures/` directory.
pub struct FakeTransport {
    dir: PathBuf,
    live: Option<ReqwestTransport>,
}

impl FakeTransport {
    /// The `user_agent` recordings are made with
    pub fn new(user_agent: &UserAgent) -> eyre::Result<Self> {
        let live = match std::env::var_os(RECORD) {
            Some(_) => Some(ReqwestTransport::new(user_agent)?),
            None => None,
        };
        Ok(FakeTransport {
            dir: PathBuf::from(FIXTURES),
            live,
        })
    }

    /// `{dir}/{host}/request.{hash}`.
    ///
    /// I don't want to hit filename length limits, or encode a huge URL with query parameters in
    /// the filename. So hash the request inputs, and use the hash as the filename.
    fn fixture_path(&self, url: &Url, key: &str) -> eyre::Result<PathBuf> {
        let host = url.host_str().ok_or_else(|| eyre!("{url} has no host"))?;
        Ok(self
            .dir
            .join(encode_host(host))
            .join(format!("request.{}", hash(key))))
    }
}

impl Transport for FakeTransport {
    fn get(
        &self,
        url: Url,
        headers: HeaderMap,
    ) -> BoxFuture<'_, eyre::Result<http::Response<Bytes>>> {
        let future = async move {
            let key = request_key(&url, &headers);
            let path = self.fixture_path(&url, &key)?;
            let Some(live) = &self.live else {
                return replay(&path);
            };

            // Under nextest, each test executes in its own process, even with --test-threads 1.
            //
            // If we don't add a sleep here, then we're potentially hammering the server with
            // requests as fast as possible (back-to-back tests with no pause interval). This sleep
            // is the simplest way to be polite to the servers.
            tokio::time::sleep(Duration::from_secs(1)).await;
            let response = live.get(url, headers).await?;
            let status = response.status();
            if status != StatusCode::TOO_MANY_REQUESTS && !status.is_server_error() {
                record(&path, &key, &response)
                    .wrap_err_with(|| format!("Failed to record {}", path.display()))?;
            }
            Ok(response)
        };
        Box::pin(future)
    }
}

/// Encode the host to a legal directory name
fn encode_host(host: &str) -> String {
    let mut encoded = String::with_capacity(host.len());
    for byte in host.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// Enough of the SHA-256 of a request key to distinguish the requests one endpoint answers.
fn hash(key: &str) -> String {
    Sha256::digest(key.as_bytes())[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Writes the fixture format, which is text so that any drift shows up as a readable diff
///
/// ```text
/// URL
///
/// STATUS
/// HEADERS
/// ...
///
/// RESPONSE
/// ```
fn record(path: &Path, key: &str, response: &http::Response<Bytes>) -> eyre::Result<()> {
    let mut fixture = Vec::new();
    fixture.extend_from_slice(key.as_bytes());
    fixture.extend_from_slice(b"\n\n");
    fixture.extend_from_slice(response.status().as_str().as_bytes());
    fixture.push(b'\n');
    for (name, value) in response.headers() {
        fixture.extend_from_slice(name.as_str().as_bytes());
        fixture.extend_from_slice(b": ");
        fixture.extend_from_slice(value.as_bytes());
        fixture.push(b'\n');
    }
    fixture.push(b'\n');
    fixture.extend_from_slice(response.body());

    let dir = path
        .parent()
        .ok_or_else(|| eyre!("A fixture has a parent"))?;
    std::fs::create_dir_all(dir)?;
    std::fs::write(path, fixture)?;
    Ok(())
}

fn replay(file: &Path) -> eyre::Result<http::Response<Bytes>> {
    let path = file.display();
    let fixture = std::fs::read(file).wrap_err_with(|| {
        format!("No fixture at {path}. Re-run with {RECORD}=1 to record it from the live API.")
    })?;

    let (_request, rest) =
        split_at_blank_line(&fixture).ok_or_else(|| eyre!("{path} has no request block"))?;
    let (head, body) =
        split_at_blank_line(rest).ok_or_else(|| eyre!("{path} has no response headers"))?;

    let mut lines = head.split(|&byte| byte == b'\n');
    let status = lines.next().ok_or_else(|| eyre!("{path} has no status"))?;
    let mut response = http::Response::new(Bytes::copy_from_slice(body));
    *response.status_mut() =
        StatusCode::from_bytes(status).wrap_err_with(|| format!("{path} has a bad status"))?;
    for line in lines {
        let (name, value) =
            split_once(line, b':').ok_or_else(|| eyre!("{path} has a bad header: {line:?}"))?;
        let name = HeaderName::from_bytes(name).wrap_err_with(|| format!("{path}: header name"))?;
        let value = HeaderValue::from_bytes(value.strip_prefix(b" ").unwrap_or(value))
            .wrap_err_with(|| format!("{path}: value of the {name} header"))?;
        response.headers_mut().append(name, value);
    }
    Ok(response)
}

/// Splits at the first empty line, discarding it. `None` when there isn't one.
fn split_at_blank_line(fixture: &[u8]) -> Option<(&[u8], &[u8])> {
    let blank = fixture.windows(2).position(|pair| pair == b"\n\n")?;
    Some((&fixture[..blank], &fixture[blank + 2..]))
}

fn split_once(line: &[u8], separator: u8) -> Option<(&[u8], &[u8])> {
    let at = line.iter().position(|&byte| byte == separator)?;
    Some((&line[..at], &line[at + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake() -> FakeTransport {
        let user_agent = UserAgent {
            app: env!("CARGO_PKG_NAME").to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            contact: Some("Notgnoshi@gmail.com".to_string()),
        };
        FakeTransport::new(&user_agent).unwrap()
    }

    /// Exercises the whole record and replay round trip
    #[tokio::test]
    async fn replays_a_recorded_response() {
        let url = Url::parse("https://api.github.com/repos/Notgnoshi/scorarium").unwrap();
        let response = fake().get(url, HeaderMap::new()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let repository: serde_json::Value = serde_json::from_slice(response.body()).unwrap();
        assert_eq!(repository["full_name"], "Notgnoshi/scorarium");
    }

    /// If the hashing doesn't include all the inputs, that's a really painful bug.
    /// And it also potentially leads to fixture files that don't get overwritten when a new
    /// response is recorded.
    #[test]
    fn fixture_names_are_the_host_and_the_request_key() {
        let fake = fake();
        let search = Url::parse("https://openlibrary.org/search.json?title=rondos").unwrap();
        let path = |headers: &HeaderMap| {
            let path = fake
                .fixture_path(&search, &request_key(&search, headers))
                .unwrap();
            path.strip_prefix(&fake.dir).unwrap().to_owned()
        };
        let mut json = HeaderMap::new();
        json.insert(http::header::ACCEPT, HeaderValue::from_static("text/json"));

        let bare = path(&HeaderMap::new());

        assert_eq!(bare, Path::new("openlibrary.org/request.687f805feb938cfc"));
        assert_ne!(path(&json), bare);
    }
}
