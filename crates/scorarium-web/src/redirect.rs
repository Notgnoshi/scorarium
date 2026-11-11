use std::convert::Infallible;

use axum::extract::FromRequestParts;
use axum::http::Uri;
use axum::http::request::Parts;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;

/// A query string of a page that can send the user back to where they came from
#[derive(Debug, Default, Deserialize)]
pub(crate) struct BackQuery {
    pub back: Option<String>,
    /// The page to go to after a successful save, overriding `back`
    pub next: Option<String>,
}

/// Keep `path` only when it stays on this site
pub(crate) fn same_site_path(path: Option<String>) -> Option<String> {
    // Browsers parse `\` as `/` and strip tabs and newlines in URLs, so `/\evil.com` and
    // `/<TAB>/evil.com` resolve to another site just as `//evil.com` does.
    path.filter(|path| {
        path.starts_with('/')
            && !path[1..].starts_with(['/', '\\'])
            && !path.contains(|c: char| c.is_ascii_control())
    })
}

/// Where an edit page returns to
pub(crate) struct Back {
    /// The page the opened this page
    opener: Option<String>,
    next: Option<String>,
    default: String,
}

impl Back {
    /// Keep the opener of `query` when it stays on this site, and fall back to `default`
    pub fn new(query: BackQuery, default: String) -> Back {
        Back {
            opener: same_site_path(query.back),
            next: same_site_path(query.next),
            default,
        }
    }

    /// Get the path to return to
    pub fn path(&self) -> &str {
        self.opener.as_deref().unwrap_or(&self.default)
    }

    /// Get the path to go to after a successful save: the page being left for, or else `path()`
    pub fn path_after_save(&self) -> &str {
        self.next.as_deref().unwrap_or_else(|| self.path())
    }

    /// Get the path to return to, or the default when the opener is `gone` or beneath it
    pub fn path_outside(&self, gone: &str) -> &str {
        let Some(opener) = &self.opener else {
            return &self.default;
        };
        let opener_path = opener
            .split_once('?')
            .map_or(opener.as_str(), |(path, _)| path);
        match opener_path.strip_prefix(gone) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => &self.default,
            _ => opener,
        }
    }

    /// Write the query string that carries the opener to another route, empty with no opener
    pub fn query(&self) -> String {
        match &self.opener {
            Some(opener) => back_query(opener),
            None => String::new(),
        }
    }
}

/// Write the query string that carries `opener` to another route
pub(crate) fn back_query(opener: &str) -> String {
    format!("?back={}", utf8_percent_encode(opener, NON_ALPHANUMERIC))
}

/// The same-site path and query of the page the browser shows, from htmx's `HX-Current-URL`
pub(crate) struct CurrentPage(pub Option<String>);

impl<S: Send + Sync> FromRequestParts<S> for CurrentPage {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Infallible> {
        let path = parts
            .headers
            .get("HX-Current-URL")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<Uri>().ok())
            .and_then(|uri| uri.path_and_query().map(ToString::to_string));
        Ok(CurrentPage(same_site_path(path)))
    }
}

impl CurrentPage {
    /// Read the `?back=` and `?next=` that the page carries in its query string
    pub fn query(&self) -> BackQuery {
        self.0
            .as_deref()
            .and_then(|page| page.split_once('?'))
            .and_then(|(_, query)| serde_html_form::from_str(query).ok())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_site_path_rejects_paths_that_leave_the_site() {
        for (path, expected) in [
            ("/library/1", true),
            ("//example.com", false),
            ("/\\example.com", false),
            ("/\t/example.com", false),
            ("https://example.com", false),
            ("", false),
        ] {
            assert_eq!(
                same_site_path(Some(path.to_owned())).is_some(),
                expected,
                "{path:?}"
            );
        }
    }

    #[test]
    fn path_outside_falls_back_only_beneath_gone() {
        for (opener, expected) in [
            ("/library/1/work/1", "/library/1"),
            ("/library/1/work/1/edit?back=/review", "/library/1"),
            ("/library/1/work/12", "/library/1/work/12"),
            (
                "/library/1/publication/3/edit",
                "/library/1/publication/3/edit",
            ),
        ] {
            let back = Back::new(
                BackQuery {
                    back: Some(opener.to_owned()),
                    next: None,
                },
                "/library/1".to_owned(),
            );
            assert_eq!(back.path_outside("/library/1/work/1"), expected, "{opener}");
        }
    }
}
