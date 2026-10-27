use std::fmt;
use std::num::ParseIntError;
use std::str::FromStr;

use url::Url;

use crate::{Error, Result};

/// A HTTP/S web address
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HttpUrl(Url);

impl HttpUrl {
    pub fn as_url(&self) -> &Url {
        &self.0
    }
}

impl TryFrom<Url> for HttpUrl {
    type Error = Error;

    fn try_from(url: Url) -> Result<HttpUrl> {
        if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() {
            Ok(HttpUrl(url))
        } else {
            Err(Error::InvalidValue(format!(
                "Failed to parse '{url}' as a web address."
            )))
        }
    }
}

impl FromStr for HttpUrl {
    type Err = Error;

    fn from_str(input: &str) -> Result<HttpUrl> {
        let input = input.trim();
        let url = Url::parse(input).map_err(|_| {
            Error::InvalidValue(format!("Failed to parse '{input}' as a web address."))
        })?;
        HttpUrl::try_from(url)
    }
}

impl fmt::Display for HttpUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LinkId(pub(crate) i64);

impl fmt::Display for LinkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for LinkId {
    type Err = ParseIntError;

    fn from_str(text: &str) -> std::result::Result<LinkId, ParseIntError> {
        text.parse().map(LinkId)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_url_accepts_only_web_addresses() {
        for (input, canonical) in [
            ("  https://IMSLP.org  ", "https://imslp.org/"),
            ("https://imslp.org:443/", "https://imslp.org/"),
            ("HTTPS://Example.com", "https://example.com/"),
        ] {
            assert_eq!(input.parse::<HttpUrl>().unwrap().to_string(), canonical);
        }
        for input in [
            "imslp.org",
            "javascript:alert(1)",
            "file://server/share/score.pdf",
            "http://",
            "",
        ] {
            assert!(
                matches!(input.parse::<HttpUrl>(), Err(Error::InvalidValue(_))),
                "{input:?}"
            );
        }
    }
}
