use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;

/// A query string of a page that can send the user back to where they came from
#[derive(Debug, Deserialize)]
pub(crate) struct BackQuery {
    pub back: Option<String>,
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
    default: String,
}

impl Back {
    /// Keep the opener of `query` when it stays on this site, and fall back to `default`
    pub fn new(query: BackQuery, default: String) -> Back {
        Back {
            opener: same_site_path(query.back),
            default,
        }
    }

    /// Get the path to return to
    pub fn path(&self) -> &str {
        self.opener.as_deref().unwrap_or(&self.default)
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
                },
                "/library/1".to_owned(),
            );
            assert_eq!(back.path_outside("/library/1/work/1"), expected, "{opener}");
        }
    }
}
