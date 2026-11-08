use std::fmt;

use scorarium_engine::entity::EntityKind;
use scorarium_engine::value::Classification;

/// An icon rendered as a Bootstrap glyph with a tooltip
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Icon {
    Person,
    Publication,
    Work,
    MusicWork,
    ProseWork,
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
}

impl fmt::Display for Icon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (class, tooltip) = match self {
            Icon::Person => ("bi-person", "Person"),
            Icon::Publication => ("bi-book", "Publication"),
            Icon::Work => ("bi-file-earmark", "Work"),
            Icon::MusicWork => ("bi-file-earmark-music", "Sheet music"),
            Icon::ProseWork => ("bi-file-earmark-text", "Prose"),
        };
        write!(f, r#"<i class="bi {class}" title="{tooltip}"></i>"#)
    }
}

impl askama::filters::HtmlSafe for Icon {}
