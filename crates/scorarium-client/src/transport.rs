use std::future::Future;
use std::pin::Pin;

use bytes::Bytes;
use eyre::WrapErr;
use http::HeaderMap;
use url::Url;

use crate::UserAgent;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// An HTTP transport layer that allows dependency injecting fakes for tests.
pub trait Transport: Send + Sync {
    /// Perform a GET request
    ///
    /// The [Transport]s User-Agent will automatically be applied.
    ///
    /// Returns error for transport failures (DNS, connection, TLS, etc). HTTP errors are returned
    /// as successful [http::Response]s.
    fn get(
        &self,
        url: Url,
        headers: HeaderMap,
    ) -> BoxFuture<'_, eyre::Result<http::Response<Bytes>>>;
}

/// Get a cache key for the given request.
///
/// If the implementation of this function changes, it will likely lead to stale files in
/// `fixtures/` that will need to be manually cleaned up.
pub(crate) fn request_key(url: &Url, headers: &HeaderMap) -> String {
    let mut lines: Vec<String> = headers
        .iter()
        .map(|(name, value)| format!("{name}: {}", String::from_utf8_lossy(value.as_bytes())))
        .collect();
    lines.sort();

    let mut key = url.to_string();
    for line in lines {
        key.push('\n');
        key.push_str(&line);
    }
    key
}

/// A real HTTP client with the User-Agent applied to every request.
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub fn new(user_agent: &UserAgent) -> eyre::Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(user_agent.header()?)
            .build()
            .wrap_err("Failed to build the HTTP client")?;
        Ok(ReqwestTransport { client })
    }
}

impl Transport for ReqwestTransport {
    fn get(
        &self,
        url: Url,
        headers: HeaderMap,
    ) -> BoxFuture<'_, eyre::Result<http::Response<Bytes>>> {
        let request = self.client.get(url.clone()).headers(headers);
        let future = async move {
            let mut response = request
                .send()
                .await
                .wrap_err_with(|| format!("GET {url}"))?;
            let status = response.status();
            // Taking the headers avoids a clone; bytes() consumes the response.
            let headers = std::mem::take(response.headers_mut());
            let body = response
                .bytes()
                .await
                .wrap_err_with(|| format!("Failed to read the response body of GET {url}"))?;

            let mut response = http::Response::new(body);
            *response.status_mut() = status;
            *response.headers_mut() = headers;
            Ok(response)
        };
        Box::pin(future)
    }
}
