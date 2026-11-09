use askama::Template;
use scorarium_engine::entity::Entity;
use scorarium_engine::holding::{Holding, HoldingId, HoldingKind};
use scorarium_engine::library::LibraryId;
use scorarium_engine::publication::{Publication, PublicationId};

#[derive(Template)]
#[template(path = "widget/holdings.html")]
pub(crate) struct Holdings {
    entries: Vec<HoldingEntry>,
}

impl askama::filters::HtmlSafe for Holdings {}

impl Holdings {
    pub fn new(publication: &Publication) -> Holdings {
        Holdings {
            entries: publication
                .holdings()
                .iter()
                .map(|holding| HoldingEntry::new(publication, holding))
                .collect(),
        }
    }
}

/// One copy in the holdings box
#[derive(Template)]
#[template(path = "widget/holding.html")]
pub(crate) struct HoldingEntry {
    library: LibraryId,
    publication: PublicationId,
    id: HoldingId,
    kind: Kind,
    /// Whether to show the brief "Saved" cue
    saved: bool,
}

impl askama::filters::HtmlSafe for HoldingEntry {}

enum Kind {
    Physical { location: String },
    Digital { filename: String },
}

impl HoldingEntry {
    pub fn new(publication: &Publication, holding: &Holding) -> HoldingEntry {
        let kind = match &holding.kind {
            HoldingKind::Physical { location } => Kind::Physical {
                location: location.clone().unwrap_or_default(),
            },
            HoldingKind::Digital { file } => Kind::Digital {
                filename: file
                    .file_name()
                    .unwrap_or(file.as_os_str())
                    .to_string_lossy()
                    .into_owned(),
            },
        };
        HoldingEntry {
            library: publication.library(),
            publication: publication.id(),
            id: holding.id,
            kind,
            saved: false,
        }
    }

    /// Show the "Saved" cue on the entry
    pub fn saved(mut self) -> HoldingEntry {
        self.saved = true;
        self
    }
}
