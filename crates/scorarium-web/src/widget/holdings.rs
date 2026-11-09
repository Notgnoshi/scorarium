use askama::Template;
use scorarium_engine::entity::Entity;
use scorarium_engine::holding::{Holding, HoldingId, HoldingKind};
use scorarium_engine::library::LibraryId;
use scorarium_engine::publication::{Publication, PublicationId};

use crate::widget::typeahead::Typeahead;

#[derive(Template)]
#[template(path = "widget/holdings.html")]
pub(crate) struct Holdings {
    entries: Vec<HoldingEntry>,
    add: HoldingAdd,
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
            add: HoldingAdd::new(publication, false),
        }
    }
}

/// The collapsed form beneath the holdings that adds another copy
#[derive(Template)]
#[template(path = "widget/holding_add.html")]
pub(crate) struct HoldingAdd {
    library: LibraryId,
    publication: PublicationId,
    /// Whether to mark the form as an htmx out-of-band replacement of the one on the page
    oob: bool,
}

impl askama::filters::HtmlSafe for HoldingAdd {}

impl HoldingAdd {
    pub fn new(publication: &Publication, oob: bool) -> HoldingAdd {
        HoldingAdd {
            library: publication.library(),
            publication: publication.id(),
            oob,
        }
    }

    fn location_input(&self) -> Typeahead {
        location_input(self.library, "holding-add-location", "")
    }
}

/// Build a location field, which suggests the library's existing locations
fn location_input(library: LibraryId, id: &str, location: &str) -> Typeahead {
    Typeahead {
        id: id.to_owned(),
        name: "location".to_owned(),
        url: format!("/api/library/{library}/suggest/location"),
        placeholder: "Location".to_owned(),
        value: location.to_owned(),
        picked: None,
        pick_required: None,
        attrs: vec![("aria-label", "Location".to_owned())],
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
        }
    }

    fn location_input(&self, location: &str) -> Typeahead {
        location_input(
            self.library,
            &format!("holding-{}-location", self.id),
            location,
        )
    }
}
