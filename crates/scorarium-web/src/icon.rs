use std::fmt;

use scorarium_engine::entity::EntityKind;
use scorarium_engine::link::{Link, LinkKind};
use scorarium_engine::value::Classification;

/// An icon rendered as a Bootstrap glyph with a tooltip
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    Person,
    Publication,
    Work,
    MusicWork,
    ProseWork,
    Wikipedia,
    Link,
    /// Vendored SVG markup
    Svg(&'static str),
    /// A host to fetch a favicon.ico from
    Favicon(String),
}

impl Icon {
    /// Get the kind icon for an entity
    pub fn kind(kind: EntityKind, classification: Option<Classification>) -> Icon {
        match kind {
            EntityKind::Person => Icon::Person,
            EntityKind::Publication => Icon::Publication,
            EntityKind::Work => Icon::work(classification),
        }
    }

    /// Get the kind icon for a work
    pub fn work(classification: Option<Classification>) -> Icon {
        match classification {
            Some(Classification::Music) => Icon::MusicWork,
            Some(Classification::Prose) => Icon::ProseWork,
            None => Icon::Work,
        }
    }

    /// Get the icon for a link's site
    pub fn link(link: &Link) -> Icon {
        let Some(kind) = link.record.as_ref().map(|record| record.kind()) else {
            let host = link.url.as_url().host_str().unwrap_or_default();
            return Icon::Favicon(host.to_owned());
        };
        match kind {
            LinkKind::Imslp => Icon::Svg(include_str!("../icons/imslp.svg")),
            LinkKind::Wikidata => Icon::Svg(include_str!("../icons/wikidata.svg")),
            LinkKind::MusicBrainz => Icon::Svg(include_str!("../icons/musicbrainz.svg")),
            LinkKind::InternetArchive => Icon::Svg(include_str!("../icons/internetarchive.svg")),
            LinkKind::OpenLibrary => Icon::Svg(include_str!("../icons/openlibrary.svg")),
            LinkKind::Goodreads => Icon::Svg(include_str!("../icons/goodreads.svg")),
            LinkKind::LibraryThing => Icon::Svg(include_str!("../icons/librarything.svg")),
            LinkKind::Wikipedia => Icon::Wikipedia,
            LinkKind::Viaf => Icon::Favicon("viaf.org".to_owned()),
            LinkKind::Isni => Icon::Favicon("isni.org".to_owned()),
            LinkKind::Loc => Icon::Favicon("www.loc.gov".to_owned()),
            LinkKind::Gnd | LinkKind::Dnb => Icon::Favicon("d-nb.info".to_owned()),
            LinkKind::Harvard => Icon::Favicon("id.lib.harvard.edu".to_owned()),
            LinkKind::Gutenberg => Icon::Favicon("www.gutenberg.org".to_owned()),
            LinkKind::Librivox => Icon::Favicon("librivox.org".to_owned()),
            // K10plus serves an empty favicon, which would leave a blank space with no fallback.
            LinkKind::K10plus => Icon::Link,
        }
    }
}

impl fmt::Display for Icon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (class, tooltip) = match self {
            Icon::Person => ("bi-person", "Person"),
            Icon::Publication => ("bi-book", "Publication"),
            Icon::Work => ("bi-file-earmark", "Work"),
            Icon::MusicWork => ("bi-file-earmark-music", "Sheet music"),
            Icon::ProseWork => ("bi-file-earmark-text", "Prose"),
            Icon::Wikipedia => ("bi-wikipedia", "Wikipedia"),
            Icon::Link => ("bi-link-45deg", "Link"),
            Icon::Svg(markup) => return f.write_str(markup),
            Icon::Favicon(host) => {
                // A URL's host may contain quotes, which would otherwise close the attribute.
                let Ok(host) = askama::filters::escape(host, askama::filters::Html);
                // The glyph shows only when the favicon fails to load; see common.css.
                write!(
                    f,
                    r#"<img class="link-favicon" src="https://{host}/favicon.ico" alt="" referrerpolicy="no-referrer" onerror="this.hidden = true">"#
                )?;
                return Icon::Link.fmt(f);
            }
        };
        write!(f, r#"<i class="bi {class}" title="{tooltip}"></i>"#)
    }
}

impl askama::filters::HtmlSafe for Icon {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favicon_escapes_the_host() {
        let html = Icon::Favicon(r#"a"onerror="alert(1)"b.com"#.to_owned()).to_string();
        assert!(
            html.contains(r#"src="https://a&#34;onerror=&#34;alert(1)&#34;b.com/favicon.ico""#),
            "{html}"
        );
    }
}
