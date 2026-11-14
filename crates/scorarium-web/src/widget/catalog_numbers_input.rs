use askama::Template;
use scorarium_engine::work::CatalogNumber;
use serde::Deserialize;

use crate::forms;
use crate::proposal::{self, Candidate};
use crate::widget::list::Edits;
use crate::widget::refusals::Refusals;

#[derive(Clone, Deserialize)]
struct CatalogNumberFields {
    value: String,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct CatalogNumberEntry {
    saved: Option<CatalogNumberFields>,
    removed: bool,
    value: String,
    proposal: Vec<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct CatalogNumbersForm {
    #[serde(deserialize_with = "forms::entries")]
    catalog_number: Vec<CatalogNumberEntry>,
}

impl CatalogNumbersForm {
    pub fn parse(&self, refusals: &mut Refusals) -> Edits<CatalogNumber, CatalogNumber> {
        let mut edits = Edits {
            remove: vec![],
            add: vec![],
        };
        for (n, entry) in self.catalog_number.iter().enumerate() {
            let saved = match &entry.saved {
                Some(saved) => match refusals.check(saved.value.parse()) {
                    Some(address) => Some((address, saved.value.trim() != entry.value.trim())),
                    None => continue,
                },
                None => None,
            };
            let field = format!("catalog_number[{n}][value]");
            edits.entry(saved, entry.removed, &entry.value, |text| {
                refusals.parse(&field, text)
            });
        }
        edits
    }
}

impl From<&[CatalogNumber]> for CatalogNumbersForm {
    fn from(numbers: &[CatalogNumber]) -> Self {
        let catalog_number = numbers
            .iter()
            .map(|number| CatalogNumberEntry {
                saved: Some(CatalogNumberFields {
                    value: number.as_str().to_owned(),
                }),
                removed: false,
                value: number.as_str().to_owned(),
                proposal: vec![],
            })
            .collect();
        CatalogNumbersForm { catalog_number }
    }
}

#[derive(Template)]
#[template(path = "widget/catalog_number_candidates.html")]
pub(crate) struct CatalogNumberCandidates {
    candidates: Vec<Candidate<CatalogNumber>>,
}

impl CatalogNumberCandidates {
    pub fn new(candidates: Vec<Candidate<CatalogNumber>>) -> CatalogNumberCandidates {
        CatalogNumberCandidates { candidates }
    }
}

impl askama::filters::HtmlSafe for CatalogNumberCandidates {}

#[derive(Default)]
struct EntryInput {
    entry: CatalogNumberEntry,
    unrecognized: bool,
    refusal: Option<String>,
    sources: Option<String>,
}

#[derive(Template)]
#[template(path = "widget/catalog_numbers_input.html")]
pub struct CatalogNumbersInput {
    entries: Vec<EntryInput>,
    blank: EntryInput,
    candidates: CatalogNumberCandidates,
}

impl askama::filters::HtmlSafe for CatalogNumbersInput {}

impl CatalogNumbersInput {
    /// Draw the entries of `form` beside the stored `numbers` and the refusals of a Save
    pub fn new(
        form: &CatalogNumbersForm,
        numbers: &[CatalogNumber],
        refusals: &Refusals,
        candidates: CatalogNumberCandidates,
    ) -> CatalogNumbersInput {
        let entries = form
            .catalog_number
            .iter()
            .enumerate()
            .map(|(n, entry)| {
                let field = format!("catalog_number[{n}][value]");
                let value = entry.value.trim();
                EntryInput {
                    entry: entry.clone(),
                    unrecognized: numbers
                        .iter()
                        .any(|number| number.as_str() == value && !number.is_recognized()),
                    refusal: refusals.of(&field).map(str::to_owned),
                    sources: proposal::sources(&candidates.candidates, &entry.proposal),
                }
            })
            .collect();
        CatalogNumbersInput {
            entries,
            blank: EntryInput::default(),
            candidates,
        }
    }
}
