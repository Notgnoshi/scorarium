use askama::Template;
use scorarium_engine::entity::Entity;
use scorarium_engine::holding::{Holding, HoldingId, HoldingKind};
use scorarium_engine::library::LibraryId;
use scorarium_engine::publication::{Publication, PublicationId};

use crate::widget::file_input::FileInput;
use crate::widget::typeahead::Typeahead;

#[derive(Template)]
#[template(path = "widget/holdings.html")]
pub(crate) struct Holdings {
    entries: Vec<HoldingEntry>,
    add: HoldingAdd,
}

impl askama::filters::HtmlSafe for Holdings {}

impl Holdings {
    pub fn new(publication: &Publication, files_allowed: bool) -> Holdings {
        Holdings {
            entries: publication
                .holdings()
                .iter()
                .map(|holding| HoldingEntry::new(publication, holding))
                .collect(),
            add: HoldingAdd::new(publication, files_allowed),
        }
    }
}

/// The collapsed form beneath the holdings that adds another copy
#[derive(Template)]
#[template(path = "widget/holding_add.html")]
pub(crate) struct HoldingAdd {
    library: LibraryId,
    publication: PublicationId,
    /// Whether a visitor may store files, which enables the Digital side
    files_allowed: bool,
}

impl askama::filters::HtmlSafe for HoldingAdd {}

impl HoldingAdd {
    pub fn new(publication: &Publication, files_allowed: bool) -> HoldingAdd {
        HoldingAdd {
            library: publication.library(),
            publication: publication.id(),
            files_allowed,
        }
    }

    fn physical(&self) -> HoldingAddPhysical {
        HoldingAddPhysical::new(self.library, self.publication, false)
    }

    fn file_input(&self) -> FileInput {
        FileInput {
            url: format!(
                "/api/library/{}/publication/{}/holding/upload",
                self.library, self.publication
            ),
            accept: None,
            hint: "Paste a file or URL",
            disabled: !self.files_allowed,
        }
    }
}

/// The form that adds a physical copy
///
/// The Digital side is left out so that replacing this form never detaches a running transfer.
#[derive(Template)]
#[template(path = "widget/holding_add_physical.html")]
pub(crate) struct HoldingAddPhysical {
    library: LibraryId,
    publication: PublicationId,
    /// Whether to mark the form as an htmx out-of-band replacement of the one on the page
    oob: bool,
    typeahead: Typeahead,
}

impl askama::filters::HtmlSafe for HoldingAddPhysical {}

impl HoldingAddPhysical {
    pub fn new(library: LibraryId, publication: PublicationId, oob: bool) -> HoldingAddPhysical {
        HoldingAddPhysical {
            library,
            publication,
            oob,
            typeahead: location_input(library, "holding-add-location", ""),
        }
    }

    /// Build a fresh form that replaces the one on the page, which clears its location
    pub fn replacement(publication: &Publication) -> HoldingAddPhysical {
        HoldingAddPhysical::new(publication.library(), publication.id(), true)
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
