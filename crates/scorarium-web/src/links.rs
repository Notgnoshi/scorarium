/// How to draw a link's icon
#[derive(Clone, Copy)]
pub enum Icon {
    /// Inline SVG markup, mostly in the foreground text color
    ///
    /// Some are square marks and some are wordmarks, so the page sizes them by height.
    Svg(&'static str),
    /// A Bootstrap Icons class, without the "bi-" prefix
    Bootstrap(&'static str),
    /// Let the browser try https://{host}/favicon.ico, falling back to the generic icon
    Favicon,
}

/// What a link's host is, as a page shows it
pub struct Site {
    /// What to print beside the icon: the site's name, else the host without a leading "www."
    pub label: String,
    /// Where to ask for a favicon, which is all [Icon::Favicon] needs from the URL
    pub host: String,
    pub icon: Icon,
}

/// Sites worth supporting explicitly, matched on the host or any subdomain of it.
const SITES: &[(&str, &str, Icon)] = &[
    (
        "imslp.org",
        "IMSLP",
        Icon::Svg(include_str!("../icons/imslp.svg")),
    ),
    ("wikipedia.org", "Wikipedia", Icon::Bootstrap("wikipedia")),
    (
        "wikidata.org",
        "Wikidata",
        Icon::Svg(include_str!("../icons/wikidata.svg")),
    ),
    (
        "musicbrainz.org",
        "MusicBrainz",
        Icon::Svg(include_str!("../icons/musicbrainz.svg")),
    ),
    (
        "archive.org",
        "Internet Archive",
        Icon::Svg(include_str!("../icons/internetarchive.svg")),
    ),
    (
        "openlibrary.org",
        "Open Library",
        Icon::Svg(include_str!("../icons/openlibrary.svg")),
    ),
    (
        "goodreads.com",
        "Goodreads",
        Icon::Svg(include_str!("../icons/goodreads.svg")),
    ),
    (
        "librarything.com",
        "LibraryThing",
        Icon::Svg(include_str!("../icons/librarything.svg")),
    ),
    ("gutenberg.org", "Project Gutenberg", Icon::Favicon),
    ("github.com", "GitHub", Icon::Bootstrap("github")),
    // fall back on the site's favicon.ico for other sites
];

impl Site {
    pub fn new(url: &str) -> Site {
        let Some(host) = url::Url::parse(url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_lowercase))
        else {
            return Site {
                label: url.to_string(),
                host: String::new(),
                icon: Icon::Bootstrap("link-45deg"),
            };
        };
        let known = SITES
            .iter()
            .find(|(domain, _, _)| host == *domain || host.ends_with(&format!(".{domain}")));
        match known {
            Some((_, label, icon)) => Site {
                label: (*label).to_string(),
                host,
                icon: *icon,
            },
            None => Site {
                label: host.strip_prefix("www.").unwrap_or(&host).to_string(),
                host,
                icon: Icon::Favicon,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn described(url: &str) -> (String, String, String) {
        let site = Site::new(url);
        let icon = match site.icon {
            Icon::Svg(_) => "svg".to_string(),
            Icon::Bootstrap(class) => format!("bi-{class}"),
            Icon::Favicon => "favicon".to_string(),
        };
        (site.label, site.host, icon)
    }

    #[test]
    fn a_known_site_is_named_and_anything_else_shows_its_host() {
        // Every language edition matches the one entry
        assert_eq!(
            described("https://en.wikipedia.org/wiki/Sergei_Rachmaninoff"),
            (
                "Wikipedia".into(),
                "en.wikipedia.org".into(),
                "bi-wikipedia".into()
            )
        );
        assert_eq!(
            described("https://www.wikidata.org/wiki/Q131861"),
            ("Wikidata".into(), "www.wikidata.org".into(), "svg".into())
        );
        assert_eq!(
            described("https://imslp.org/wiki/Category:Rachmaninoff,_Sergei"),
            ("IMSLP".into(), "imslp.org".into(), "svg".into())
        );
        // A site can be named here and still be drawn by its own favicon
        assert_eq!(
            described("https://www.gutenberg.org/ebooks/972"),
            (
                "Project Gutenberg".into(),
                "www.gutenberg.org".into(),
                "favicon".into()
            )
        );
        // A host that merely ends in a known name is not that site
        assert_eq!(
            described("https://notimslp.org/"),
            (
                "notimslp.org".into(),
                "notimslp.org".into(),
                "favicon".into()
            )
        );
        // An unknown site is its host, minus the "www." nobody reads
        assert_eq!(
            described("https://www.henle.de/en/"),
            ("henle.de".into(), "www.henle.de".into(), "favicon".into())
        );
    }
}
