use std::path::PathBuf;
use std::time::Duration;

use bytes::Bytes;
use eyre::eyre;
use http::{HeaderMap, StatusCode};
use url::Url;

use crate::transport::{BoxFuture, ReqwestTransport, Transport, request_key};
use crate::{UserAgent, response_file};

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
}

impl Transport for FakeTransport {
    fn get(
        &self,
        url: Url,
        headers: HeaderMap,
    ) -> BoxFuture<'_, eyre::Result<http::Response<Bytes>>> {
        let future = async move {
            let key = request_key(&url, &headers);
            let path = self.dir.join(response_file::relative_path(&url, &key)?);
            let Some(live) = &self.live else {
                let (_key, response) = response_file::read(&path)?.ok_or_else(|| {
                    eyre!(
                        "No fixture at {}. Re-run with {RECORD}=1 to record it from the live API.",
                        path.display()
                    )
                })?;
                return Ok(response);
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
                response_file::write(&path, &key, &response)?;
            }
            Ok(response)
        };
        Box::pin(future)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exercises the whole record and replay round trip
    #[tokio::test]
    async fn replays_a_recorded_response() {
        let user_agent = UserAgent::new(Some("Notgnoshi@gmail.com"));
        let fake = FakeTransport::new(&user_agent).unwrap();
        let url = Url::parse("https://api.github.com/repos/Notgnoshi/scorarium").unwrap();
        let response = fake.get(url, HeaderMap::new()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let repository: serde_json::Value = serde_json::from_slice(response.body()).unwrap();
        assert_eq!(repository["full_name"], "Notgnoshi/scorarium");
    }
}
