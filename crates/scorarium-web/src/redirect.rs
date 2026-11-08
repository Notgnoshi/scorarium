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
}
