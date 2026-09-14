#[cfg(feature = "fake-transport")]
pub mod fake;
pub mod open_library;
mod transport;

use std::sync::Arc;

use bytes::Bytes;
use eyre::WrapErr;
use http::{HeaderMap, HeaderValue};
use url::Url;

use crate::open_library::OpenLibrary;
pub use crate::transport::{BoxFuture, ReqwestTransport, Transport};

/// Identifies scorarium to the sources.
///
/// Some APIs change their rate limits based on whether there's contact info in the User-Agent
#[derive(Clone)]
pub struct UserAgent {
    pub app: String,
    pub version: String,
    /// Optional but recommended; some APIs give increased rate limits when it's provided
    pub contact: Option<String>,
}

impl UserAgent {
    /// `{app}/{version}` or `{app}/{version} ({contact})`
    pub fn header(&self) -> eyre::Result<HeaderValue> {
        let value = match &self.contact {
            Some(contact) => format!("{}/{} ({contact})", self.app, self.version),
            None => format!("{}/{}", self.app, self.version),
        };
        HeaderValue::from_str(&value).wrap_err_with(|| format!("Invalid User-Agent: {value}"))
    }
}

/// The handle every source client is reached through.
pub struct Client {
    transport: Arc<dyn Transport>,
}

impl Client {
    pub fn new(user_agent: UserAgent) -> eyre::Result<Client> {
        let transport = ReqwestTransport::new(&user_agent)?;
        Ok(Client::with_transport(user_agent, Arc::new(transport)))
    }

    /// Builds a client over a transport of the caller's choosing
    ///
    /// The UserAgent is still required because the API clients can base their rate limits on it.
    pub fn with_transport(_user_agent: UserAgent, transport: Arc<dyn Transport>) -> Client {
        Client { transport }
    }

    /// Get an API client for [Open Library](https://openlibrary.org)
    pub fn open_library(&self) -> OpenLibrary<'_> {
        OpenLibrary::new(self)
    }

    pub async fn get(&self, url: Url) -> eyre::Result<http::Response<Bytes>> {
        self.transport.get(url, HeaderMap::new()).await
    }
}
