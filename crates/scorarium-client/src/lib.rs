mod cache;
#[cfg(feature = "fake-transport")]
pub mod fake;
pub mod open_library;
mod rate_limited_client;
mod transport;

use std::sync::Arc;

use eyre::WrapErr;
use http::HeaderValue;

use crate::cache::Cache;
use crate::open_library::OpenLibrary;
use crate::rate_limited_client::RateLimitedClient;
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

/// How a request is scheduled
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    /// Interactive requests are pushed to the front of the queue, and are never retried. They're
    /// meant for interactive use-cases.
    ///
    /// Any queued interactive requests that haven't been started yet will be canceled. The
    /// assumption is that interactive requests are for progressive refinement of the same query,
    /// and the caller only wants responses for an in-flight request, and the latest request, and no
    /// requests in between.
    Interactive,
    /// Background requests are pushed to the back of the queue, and are retried with backoff if the
    /// request gets rate limited.
    Background,
}

/// The handle every source client is reached through.
pub struct Client {
    cache: Arc<Cache>,
    open_library: RateLimitedClient,
}

impl Client {
    pub fn new(user_agent: UserAgent) -> eyre::Result<Client> {
        let transport = ReqwestTransport::new(&user_agent)?;
        Ok(Client::with_transport(user_agent, Arc::new(transport)))
    }

    /// Builds a client over a transport of the caller's choosing
    ///
    /// The UserAgent is still required because the API clients can base their rate limits on it.
    pub fn with_transport(user_agent: UserAgent, transport: Arc<dyn Transport>) -> Client {
        let cache = Arc::new(Cache::default());
        Client {
            open_library: RateLimitedClient::spawn(
                transport.clone(),
                open_library::limits(&user_agent),
                cache.clone(),
            ),
            cache,
        }
    }

    /// Get an API client for [Open Library](https://openlibrary.org)
    pub fn open_library(&self) -> OpenLibrary<'_> {
        OpenLibrary::new(&self.open_library)
    }

    /// Forgets every response fetched so far.
    ///
    /// Responses are kept for the lifetime of the process, so this is the only way to pick up a
    /// correction made upstream, or to prevent running out of memory. Scorarium doesn't attempt to
    /// keep its metadata up-to-date against the open APIs; only setting the metadata at import time.
    pub fn clear_cache(&self) {
        self.cache.clear();
    }
}
