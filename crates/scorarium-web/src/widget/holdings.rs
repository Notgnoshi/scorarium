use askama::Template;
use indexmap::IndexMap;
use scorarium_engine::Engine;
use scorarium_engine::asset::{AssetSource, StagedId};
use scorarium_engine::entity::Entity;
use scorarium_engine::holding::{Holding, HoldingId, HoldingKind, NewHolding};
use scorarium_engine::library::LibraryId;
use scorarium_engine::publication::Publication;
use serde::{Deserialize, Serialize};

use crate::forms;
use crate::widget::file_input::FileInput;
use crate::widget::refusals::Refusals;
use crate::widget::typeahead::Typeahead;

#[derive(Clone, Deserialize)]
struct SavedHolding {
    id: String,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    #[default]
    Physical,
    Digital,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct HoldingEntry {
    saved: Option<SavedHolding>,
    removed: bool,
    kind: Kind,
    location: String,
    url: String,
    staged: String,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct HoldingsForm {
    #[serde(deserialize_with = "forms::entries")]
    holding: Vec<HoldingEntry>,
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

fn find<'a>(holdings: &'a [Holding], saved: &SavedHolding) -> Option<&'a Holding> {
    let id = saved.id.parse::<HoldingId>().ok()?;
    holdings.iter().find(|holding| holding.id == id)
}

fn url_field(n: usize) -> String {
    format!("holding[{n}][url]")
}

impl From<&[Holding]> for HoldingsForm {
    fn from(holdings: &[Holding]) -> Self {
        let holding = holdings
            .iter()
            .map(|holding| HoldingEntry {
                saved: Some(SavedHolding {
                    id: holding.id.to_string(),
                }),
                location: match &holding.kind {
                    HoldingKind::Physical { location } => location.clone().unwrap_or_default(),
                    HoldingKind::Digital { .. } => String::new(),
                },
                ..HoldingEntry::default()
            })
            .collect();
        HoldingsForm { holding }
    }
}

impl HoldingsForm {
    /// Hold one physical entry with no location, which an import starts with
    pub fn starting() -> HoldingsForm {
        HoldingsForm {
            holding: vec![HoldingEntry::default()],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.holding.is_empty()
    }

    /// Write the physical entries as a query string, which the next import starts with
    pub fn physical_query(&self) -> String {
        #[derive(Serialize)]
        struct Physical<'a> {
            location: &'a str,
        }
        #[derive(Serialize)]
        struct Query<'a> {
            holding: IndexMap<String, Physical<'a>>,
        }
        let holding = self
            .holding
            .iter()
            .filter(|entry| entry.saved.is_none() && entry.kind == Kind::Physical)
            .enumerate()
            .map(|(n, entry)| {
                let location = &entry.location;
                (n.to_string(), Physical { location })
            })
            .collect();
        forms::to_string(&Query { holding }).expect("Holdings fields serialize as a form")
    }

    /// Read the edits that the form makes to `holdings`
    pub fn parse(
        &self,
        engine: &Engine,
        holdings: &[Holding],
        refusals: &mut Refusals,
    ) -> HoldingEdits {
        let mut edits = HoldingEdits {
            add: vec![],
            relocate: vec![],
            remove: vec![],
        };
        for (n, entry) in self.holding.iter().enumerate() {
            if let Some(saved) = &entry.saved {
                match find(holdings, saved) {
                    Some(holding) if entry.removed => edits.remove.push(holding.id),
                    Some(Holding {
                        id,
                        kind: HoldingKind::Physical { .. },
                        ..
                    }) => edits.relocate.push((*id, entry.location.clone())),
                    _ => {}
                }
            } else if entry.kind == Kind::Physical {
                edits.add.push(AddedHolding::Physical {
                    location: entry.location.clone(),
                });
            } else if !entry.staged.is_empty() {
                let Ok(id) = entry.staged.parse::<StagedId>();
                if engine.staged(&id).is_some() {
                    edits.add.push(AddedHolding::Digital(id));
                } else {
                    refusals.refuse_field(&url_field(n), "File no longer available.");
                }
            } else if !entry.url.trim().is_empty() {
                refusals.refuse_field(&url_field(n), "Not fetched.");
            }
        }
        let kept = holdings
            .iter()
            .filter(|holding| !edits.remove.contains(&holding.id))
            .count();
        if kept + edits.add.len() == 0 {
            refusals.refuse("A publication needs at least one holding.");
        }
        edits
    }
}

enum EntryInput {
    Physical {
        id: HoldingId,
        location: Typeahead,
        removed: bool,
    },
    Digital {
        id: HoldingId,
        filename: String,
        href: String,
        removed: bool,
    },
    New {
        digital: bool,
        location: Typeahead,
        file: Box<FileInput>,
    },
}

#[derive(Template)]
#[template(path = "widget/holdings.html")]
pub(crate) struct Holdings {
    /// Each entry with its key
    entries: Vec<(String, EntryInput)>,
    blank: EntryInput,
    files_allowed: bool,
}

impl askama::filters::HtmlSafe for Holdings {}

fn location_input(library: LibraryId, key: &str, location: &str) -> Typeahead {
    let id = format!("holding-{key}-location");
    Typeahead {
        id,
        name: format!("holding[{key}][location]"),
        url: format!("/api/library/{library}/suggest/location"),
        placeholder: "Location".to_owned(),
        value: location.to_owned(),
        picked: None,
        pick_required: None,
        attrs: vec![("aria-label", "Location".to_owned())],
    }
}

fn file_input(key: &str, files_allowed: bool) -> FileInput {
    FileInput {
        url: "/api/stage".to_owned(),
        accept: None,
        hint: "Paste a file or URL",
        disabled: !files_allowed,
        staged_name: format!("holding[{key}][staged]"),
        staged: None,
        shows_file: true,
        url_name: format!("holding[{key}][url]"),
        url_text: String::new(),
        refusal: None,
        browse_icon: None,
    }
}

impl Holdings {
    /// Draw the entries of `form` beside the holdings of `publication` and the refusals of a Save
    pub fn of(
        engine: &Engine,
        publication: &Publication,
        form: &HoldingsForm,
        refusals: &Refusals,
        files_allowed: bool,
    ) -> Holdings {
        let href = |holding: &Holding| {
            format!(
                "/library/{}/publication/{}/holding/{}",
                publication.library(),
                publication.id(),
                holding.id
            )
        };
        Holdings::build(
            engine,
            publication.library(),
            publication.holdings(),
            href,
            form,
            refusals,
            files_allowed,
        )
    }

    /// Draw the entries of `form` for a publication that does not exist yet
    pub fn new(
        engine: &Engine,
        library: LibraryId,
        form: &HoldingsForm,
        refusals: &Refusals,
        files_allowed: bool,
    ) -> Holdings {
        let href = |_: &Holding| String::new();
        Holdings::build(engine, library, &[], href, form, refusals, files_allowed)
    }

    fn build(
        engine: &Engine,
        library: LibraryId,
        holdings: &[Holding],
        href: impl Fn(&Holding) -> String,
        form: &HoldingsForm,
        refusals: &Refusals,
        files_allowed: bool,
    ) -> Holdings {
        let draw = |(n, entry): (usize, &HoldingEntry)| {
            let key = n.to_string();
            let location = location_input(library, &key, &entry.location);
            let removed = entry.removed;
            let input = match &entry.saved {
                Some(saved) => {
                    let holding = find(holdings, saved)?;
                    let id = holding.id;
                    match &holding.kind {
                        HoldingKind::Physical { .. } => EntryInput::Physical {
                            id,
                            location,
                            removed,
                        },
                        HoldingKind::Digital { file } => EntryInput::Digital {
                            id,
                            filename: file
                                .file_name()
                                .unwrap_or(file.as_os_str())
                                .to_string_lossy()
                                .into_owned(),
                            href: href(holding),
                            removed,
                        },
                    }
                }
                None => {
                    let Ok(id) = entry.staged.parse::<StagedId>();
                    let field = url_field(n);
                    EntryInput::New {
                        digital: entry.kind == Kind::Digital,
                        location,
                        file: Box::new(FileInput {
                            staged: engine.staged(&id).filter(|_| !entry.staged.is_empty()),
                            url_text: entry.url.clone(),
                            refusal: refusals.of(&field).map(str::to_owned),
                            ..file_input(&key, files_allowed)
                        }),
                    }
                }
            };
            Some((key, input))
        };
        let mut location = location_input(library, "__key__", "");
        location.attrs.push(("autofocus", String::new()));
        Holdings {
            entries: form.holding.iter().enumerate().filter_map(draw).collect(),
            blank: EntryInput::New {
                digital: false,
                location,
                file: Box::new(file_input("__key__", files_allowed)),
            },
            files_allowed,
        }
    }
}
