use std::collections::HashMap;
use std::time::Duration;

use eyre::{WrapErr, eyre};
use http::HeaderMap;
use serde::Deserialize;
use url::Url;

use crate::rate_limited_client::{Limits, RateLimitedClient};
use crate::{Priority, UserAgent};

const BASE: &str = "https://openlibrary.org/";

/// Open Library limits us to one request per second, and three per second for a request identified
/// by a User-Agent carrying contact info
pub(crate) fn limits(user_agent: &UserAgent) -> Limits {
    let min_interval = if user_agent.contact.is_some() {
        Duration::from_millis(334)
    } else {
        Duration::from_secs(1)
    };
    Limits {
        min_interval,
        timeout: Duration::from_secs(10),
    }
}

/// The Open Library API.
///
/// Open Library describes the same book at two levels: a work is the abstract book, and an edition
/// is one printing of it.
pub struct OpenLibrary<'a> {
    client: &'a RateLimitedClient,
}

impl<'a> OpenLibrary<'a> {
    pub(crate) fn new(client: &'a RateLimitedClient) -> OpenLibrary<'a> {
        OpenLibrary { client }
    }

    /// Look up the edition a bare ISBN names.
    ///
    /// The caller is expected to normalize the ISBN. The response will contain OLIDs for the
    /// authors, so the caller will need to follow this call up with one to [OpenLibrary::author]
    pub async fn edition_by_isbn(
        &self,
        isbn: &str,
        priority: Priority,
    ) -> eyre::Result<Option<Edition>> {
        let url = url(["isbn", &format!("{isbn}.json")])?;
        let raw: Option<RawEdition> = self
            .client
            .get_json(url, HeaderMap::new(), priority)
            .await?;
        Ok(raw.map(Edition::from))
    }

    /// Look up an author by an OLID like "OL127077A".
    pub async fn author(&self, olid: &str, priority: Priority) -> eyre::Result<Option<Author>> {
        let url = url(["authors", &format!("{olid}.json")])?;
        let raw: Option<RawAuthor> = self
            .client
            .get_json(url, HeaderMap::new(), priority)
            .await?;
        Ok(raw.map(Author::from))
    }
}

/// One printing of a book, as Open Library records it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edition {
    /// "OL7636066M"
    pub olid: String,
    pub title: String,
    pub subtitle: Option<String>,
    /// Author OLIDs in the edition's order; resolve them with [OpenLibrary::author].
    pub authors: Vec<String>,
    /// Free text, such as "Tony Ross (Illustrator)"
    pub contributions: Vec<String>,
    pub by_statement: Option<String>,
    pub publishers: Vec<String>,
    /// Free text, such as "July 1, 1987"
    pub publish_date: Option<String>,
    pub isbn_10: Vec<String>,
    pub isbn_13: Vec<String>,
    pub number_of_pages: Option<u32>,
    /// Cover ids for covers.openlibrary.org
    pub covers: Vec<i64>,
    /// Work OLIDs, such as "OL1258206W"
    pub works: Vec<String>,
}

/// A person Open Library credits on an edition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Author {
    /// "OL127077A"
    pub olid: String,
    pub name: String,
    pub personal_name: Option<String>,
    pub alternate_names: Vec<String>,
    pub birth_date: Option<String>,
    pub death_date: Option<String>,
    /// Identifiers in other authorities
    pub remote_ids: HashMap<String, String>,
}

fn url(segments: [&str; 2]) -> eyre::Result<Url> {
    let mut url = Url::parse(BASE).wrap_err("Failed to parse the Open Library base URL")?;
    url.path_segments_mut()
        .map_err(|()| eyre!("{BASE} cannot have path segments"))?
        .extend(segments);
    Ok(url)
}

fn olid(key: &str) -> String {
    // Open Library records refer to each other by key, "/authors/OL127077A". The OLID is the last
    // segment, and is what its own endpoint is addressed by.
    key.rsplit('/').next().unwrap_or(key).to_string()
}

#[derive(Deserialize)]
struct RawEdition {
    key: String,
    title: String,
    subtitle: Option<String>,
    #[serde(default)]
    authors: Vec<RawKey>,
    #[serde(default)]
    contributions: Vec<String>,
    by_statement: Option<String>,
    #[serde(default)]
    publishers: Vec<String>,
    publish_date: Option<String>,
    #[serde(default)]
    isbn_10: Vec<String>,
    #[serde(default)]
    isbn_13: Vec<String>,
    number_of_pages: Option<u32>,
    #[serde(default)]
    covers: Vec<i64>,
    #[serde(default)]
    works: Vec<RawKey>,
}

#[derive(Deserialize)]
struct RawAuthor {
    key: String,
    name: String,
    personal_name: Option<String>,
    #[serde(default)]
    alternate_names: Vec<String>,
    birth_date: Option<String>,
    death_date: Option<String>,
    #[serde(default)]
    remote_ids: HashMap<String, String>,
}

/// How Open Library refers to another record.
#[derive(Deserialize)]
struct RawKey {
    key: String,
}

impl From<RawEdition> for Edition {
    fn from(raw: RawEdition) -> Edition {
        Edition {
            olid: olid(&raw.key),
            title: raw.title,
            subtitle: raw.subtitle,
            authors: raw.authors.iter().map(|key| olid(&key.key)).collect(),
            contributions: raw.contributions,
            by_statement: raw.by_statement,
            publishers: raw.publishers,
            publish_date: raw.publish_date,
            isbn_10: raw.isbn_10,
            isbn_13: raw.isbn_13,
            number_of_pages: raw.number_of_pages,
            // Open Library uses -1 to mean "this record has no cover", which is not an id any
            // consumer can fetch.
            covers: raw.covers.into_iter().filter(|id| *id > 0).collect(),
            works: raw.works.iter().map(|key| olid(&key.key)).collect(),
        }
    }
}

impl From<RawAuthor> for Author {
    fn from(raw: RawAuthor) -> Author {
        Author {
            olid: olid(&raw.key),
            name: raw.name,
            personal_name: raw.personal_name,
            alternate_names: raw.alternate_names,
            birth_date: raw.birth_date,
            death_date: raw.death_date,
            remote_ids: raw.remote_ids,
        }
    }
}

#[cfg(all(test, feature = "fake-transport"))]
mod tests {
    use std::sync::Arc;

    use http::StatusCode;

    use crate::fake::FakeTransport;
    use crate::{Client, Outcome, Priority, UserAgent};

    fn client() -> Client {
        let user_agent = UserAgent {
            app: env!("CARGO_PKG_NAME").to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            contact: Some("Notgnoshi@gmail.com".to_string()),
        };
        let transport = FakeTransport::new(&user_agent).unwrap();
        Client::with_transport(user_agent, Arc::new(transport))
    }

    #[tokio::test]
    async fn a_lookup_shows_up_in_the_status_and_the_call_log() {
        let client = client();

        client
            .open_library()
            .edition_by_isbn("9780486253923", Priority::Background)
            .await
            .unwrap()
            .unwrap();

        let calls = client.call_history().calls;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].source, "Open Library");
        assert_eq!(
            calls[0].url.as_str(),
            "https://openlibrary.org/isbn/9780486253923.json"
        );
        assert_eq!(calls[0].attempt, 1);
        assert_eq!(calls[0].outcome, Outcome::Status(StatusCode::OK));

        let status = client.status();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].source, "Open Library");
        assert_eq!(status[0].queued, 0);
        assert!(!status[0].in_flight);
        assert_eq!(status[0].paused_for, None);
    }

    #[tokio::test]
    async fn an_isbn_resolves_to_an_edition() {
        let client = client();

        let edition = client
            .open_library()
            .edition_by_isbn("9780486253923", Priority::Interactive)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(edition.olid, "OL7636066M");
        assert_eq!(
            edition.title,
            "Bagatelles, Rondos and Other Shorter Works for Piano"
        );
        assert_eq!(edition.publishers, ["Dover Publications"]);
        assert_eq!(edition.isbn_13, ["9780486253923"]);
        assert_eq!(edition.authors, ["OL127077A"]);
        assert_eq!(edition.works, ["OL1258206W"]);
    }

    /// A valid ISMN is an identifier Open Library will never hold.
    ///
    /// Made-up ISBNs are no good as a negative case
    #[tokio::test]
    async fn an_unknown_identifier_is_not_an_error() {
        let client = client();

        let edition = client
            .open_library()
            .edition_by_isbn("9790201800011", Priority::Interactive)
            .await
            .unwrap();

        assert_eq!(edition, None);
    }

    #[tokio::test]
    async fn an_olid_resolves_to_an_author() {
        let client = client();

        let author = client
            .open_library()
            .author("OL127077A", Priority::Background)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(author.olid, "OL127077A");
        assert_eq!(author.name, "Ludwig van Beethoven");
        assert_eq!(author.birth_date.as_deref(), Some("1770"));
        assert_eq!(author.remote_ids.get("wikidata").unwrap(), "Q255");
    }
}
