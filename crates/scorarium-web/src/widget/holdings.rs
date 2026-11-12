use askama::Template;
use scorarium_engine::Engine;
use scorarium_engine::asset::{AssetSource, StagedAsset, StagedId};
use scorarium_engine::entity::Entity;
use scorarium_engine::holding::{Holding, HoldingId, HoldingKind, NewHolding};
use scorarium_engine::library::LibraryId;
use scorarium_engine::publication::Publication;
use serde::{Deserialize, Serialize};

use crate::forms;
use crate::widget::file_input::FileInput;
use crate::widget::refusals::Refusals;
use crate::widget::typeahead::Typeahead;

#[derive(Template)]
#[template(path = "widget/holdings.html")]
pub(crate) struct Holdings {
    library: LibraryId,
    entries: Vec<HoldingEntry>,
    /// The holdings that the form removes, which a resubmission must remove again
    removed: Vec<HoldingId>,
    kind: AddKind,
    location: String,
    files_allowed: bool,
}

impl askama::filters::HtmlSafe for Holdings {}

enum HoldingEntry {
    Physical {
        id: HoldingId,
        location: String,
    },
    Digital {
        id: HoldingId,
        filename: String,
        href: String,
    },
    /// A physical holding that the form will add when submitted
    StagedPhysical {
        location: String,
    },
    StagedDigital(StagedAsset),
}

impl Holdings {
    /// Draw an empty box, for a publication that does not exist yet
    pub fn empty(library: LibraryId, files_allowed: bool) -> Holdings {
        Holdings {
            library,
            entries: vec![],
            removed: vec![],
            kind: AddKind::default(),
            location: String::new(),
            files_allowed,
        }
    }

    /// Draw the holdings of `publication`
    pub fn of(publication: &Publication, files_allowed: bool) -> Holdings {
        let entry = |holding: &Holding| match &holding.kind {
            HoldingKind::Physical { location } => HoldingEntry::Physical {
                id: holding.id,
                location: location.clone().unwrap_or_default(),
            },
            HoldingKind::Digital { file } => HoldingEntry::Digital {
                id: holding.id,
                filename: file
                    .file_name()
                    .unwrap_or(file.as_os_str())
                    .to_string_lossy()
                    .into_owned(),
                href: format!(
                    "/library/{}/publication/{}/holding/{}",
                    publication.library(),
                    publication.id(),
                    holding.id
                ),
            },
        };
        Holdings {
            entries: publication.holdings().iter().map(entry).collect(),
            ..Holdings::empty(publication.library(), files_allowed)
        }
    }

    /// Draw the box as a submitted form left it, for a form that was not saved
    pub fn resubmitted(mut self, engine: &Engine, form: &HoldingsForm) -> Holdings {
        self.removed = form
            .remove_holding
            .iter()
            .filter_map(|id| id.parse().ok())
            .collect();
        self.entries.retain(|entry| match entry {
            HoldingEntry::Physical { id, .. } | HoldingEntry::Digital { id, .. } => {
                !self.removed.contains(id)
            }
            _ => true,
        });
        for (id, edited) in form.holding.iter().zip(&form.holding_location) {
            let Ok(id) = id.parse::<HoldingId>() else {
                continue;
            };
            for entry in &mut self.entries {
                if let HoldingEntry::Physical { id: held, location } = entry
                    && *held == id
                {
                    location.clone_from(edited);
                }
            }
        }
        let added = form
            .add_location
            .iter()
            .map(|location| HoldingEntry::StagedPhysical {
                location: location.clone(),
            });
        let staged = form.staged.iter().filter_map(|id| {
            let Ok(id) = id.parse::<StagedId>();
            engine.staged(&id).map(HoldingEntry::StagedDigital)
        });
        self.entries.extend(added.chain(staged));
        self.kind = form.kind;
        self.location.clone_from(&form.location);
        self
    }

    /// Build a location field, which suggests the library's existing locations
    fn location_input(&self, id: &str, name: &str, location: &str) -> Typeahead {
        Typeahead {
            id: id.to_owned(),
            name: name.to_owned(),
            url: format!("/api/library/{}/suggest/location", self.library),
            placeholder: "Location".to_owned(),
            value: location.to_owned(),
            picked: None,
            pick_required: None,
            attrs: vec![("aria-label", "Location".to_owned())],
        }
    }

    fn file_input(&self) -> FileInput {
        FileInput {
            url: "/api/stage".to_owned(),
            accept: None,
            hint: "Paste a file or URL",
            disabled: !self.files_allowed,
            multiple: true,
            staged_name: None,
            staged: None,
            shows_file: false,
            url_name: None,
            url_text: String::new(),
            refusal: None,
            autofocus: false,
            browse_icon: None,
        }
    }
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct HoldingsForm {
    /// The ids of the existing physical holdings, paired by position with `holding_location`
    #[serde(skip_serializing_if = "Vec::is_empty")]
    holding: Vec<String>,
    #[serde(
        deserialize_with = "forms::repeated",
        skip_serializing_if = "Vec::is_empty"
    )]
    holding_location: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    remove_holding: Vec<String>,
    #[serde(
        deserialize_with = "forms::repeated",
        skip_serializing_if = "Vec::is_empty"
    )]
    add_location: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    staged: Vec<String>,
    kind: AddKind,
    /// A location typed in the add bar and not added
    location: String,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AddKind {
    #[default]
    Physical,
    Digital,
}

pub(crate) struct HoldingEdits {
    pub add: Vec<AddedHolding>,
    pub relocate: Vec<(HoldingId, String)>,
    pub remove: Vec<HoldingId>,
}

pub(crate) enum AddedHolding {
    Physical { location: String },
    Digital(StagedId),
}

impl AddedHolding {
    pub fn as_new(&self) -> NewHolding<'_> {
        match self {
            AddedHolding::Physical { location } => NewHolding::Physical {
                location: Some(location),
            },
            AddedHolding::Digital(id) => NewHolding::Digital(AssetSource::Staged(id.clone())),
        }
    }
}

impl HoldingsForm {
    /// Return the toggle in the add bar
    pub fn kind(&self) -> AddKind {
        self.kind
    }

    /// Write the form as a query string, leaving out the staged files
    pub fn query_without_staged(&self) -> String {
        let form = HoldingsForm {
            staged: Vec::new(),
            ..self.clone()
        };
        forms::to_string(&form).expect("Holdings fields serialize as a form")
    }

    /// List the holdings that the form adds
    pub fn added(&self) -> Vec<AddedHolding> {
        let physical = |location: &String| AddedHolding::Physical {
            location: location.clone(),
        };
        // The add bar's location counts as added so that a person who types one and saves does not
        // lose it by forgetting to press the add button.
        let pending = (self.kind == AddKind::Physical && !self.location.trim().is_empty())
            .then_some(&self.location);
        self.add_location
            .iter()
            .chain(pending)
            .map(physical)
            .chain(self.staged.iter().map(|id| {
                let Ok(id) = id.parse();
                AddedHolding::Digital(id)
            }))
            .collect()
    }

    /// Read the edits that the form makes to `holdings`
    pub fn parse(
        &self,
        engine: &Engine,
        holdings: &[Holding],
        refusals: &mut Refusals,
    ) -> HoldingEdits {
        let known = |id: &String| {
            let id = id.parse::<HoldingId>().ok()?;
            holdings.iter().any(|h| h.id == id).then_some(id)
        };
        let add = self.added();
        let relocate = self
            .holding
            .iter()
            .zip(&self.holding_location)
            .filter_map(|(id, location)| Some((known(id)?, location.clone())))
            .collect();
        let remove: Vec<HoldingId> = self.remove_holding.iter().filter_map(known).collect();
        let gone = |holding: &AddedHolding| matches!(holding, AddedHolding::Digital(id) if engine.staged(id).is_none());
        if add.iter().any(gone) {
            refusals.refuse("A file is no longer available. Choose it again.");
        }
        let kept = holdings.iter().filter(|h| !remove.contains(&h.id)).count();
        if kept + add.len() == 0 {
            refusals.refuse("A publication needs at least one holding.");
        }
        HoldingEdits {
            add,
            relocate,
            remove,
        }
    }
}
