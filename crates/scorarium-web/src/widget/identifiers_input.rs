use askama::Template;
use scorarium_engine::publication::{Identifier, IdentifierKind};
use serde::Deserialize;

use crate::forms;
use crate::proposal::{self, Candidate};
use crate::widget::list::Edits;
use crate::widget::refusals::Refusals;

#[derive(Clone, Deserialize)]
struct IdentifierFields {
    kind: String,
    value: String,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct IdentifierEntry {
    saved: Option<IdentifierFields>,
    removed: bool,
    kind: String,
    value: String,
    proposal: Vec<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct IdentifiersForm {
    #[serde(deserialize_with = "forms::entries")]
    identifier: Vec<IdentifierEntry>,
}

impl IdentifiersForm {
    pub fn parse(&self, refusals: &mut Refusals) -> Edits<Identifier, Identifier> {
        let mut edits = Edits {
            remove: vec![],
            add: vec![],
        };
        for (n, entry) in self.identifier.iter().enumerate() {
            let saved = match &entry.saved {
                Some(saved) => {
                    let address = saved
                        .kind
                        .parse()
                        .and_then(|kind| Identifier::new(kind, &saved.value));
                    match refusals.check(address) {
                        Some(address) => Some((
                            address,
                            saved.kind != entry.kind || saved.value.trim() != entry.value.trim(),
                        )),
                        None => continue,
                    }
                }
                None => None,
            };
            let field = format!("identifier[{n}][value]");
            edits.entry(saved, entry.removed, &entry.value, |text| {
                let identifier = entry
                    .kind
                    .parse()
                    .and_then(|kind| Identifier::new(kind, text));
                refusals.check_field(&field, identifier)
            });
        }
        edits
    }
}

impl From<&[Identifier]> for IdentifiersForm {
    fn from(identifiers: &[Identifier]) -> Self {
        let identifier = identifiers
            .iter()
            .map(|identifier| IdentifierEntry {
                saved: Some(IdentifierFields {
                    kind: identifier.kind().to_string(),
                    value: identifier.as_str().to_owned(),
                }),
                removed: false,
                kind: identifier.kind().to_string(),
                value: identifier.as_str().to_owned(),
                proposal: vec![],
            })
            .collect();
        IdentifiersForm { identifier }
    }
}

/// Get the label shown before an identifier, if its kind needs one
pub(crate) fn kind_label(kind: IdentifierKind) -> Option<&'static str> {
    match kind {
        IdentifierKind::Isbn => Some("ISBN"),
        IdentifierKind::Ismn => Some("ISMN"),
        IdentifierKind::PlateNumber => Some("Plate No."),
        IdentifierKind::PublisherNumber => None,
    }
}

#[derive(Template)]
#[template(path = "widget/identifier_candidates.html")]
pub(crate) struct IdentifierCandidates {
    candidates: Vec<Candidate<Identifier>>,
}

impl IdentifierCandidates {
    pub fn new(candidates: Vec<Candidate<Identifier>>) -> IdentifierCandidates {
        IdentifierCandidates { candidates }
    }
}

impl askama::filters::HtmlSafe for IdentifierCandidates {}

struct EntryInput {
    entry: IdentifierEntry,
    refusal: Option<String>,
    autofocus: bool,
    sources: Option<String>,
}

#[derive(Template)]
#[template(path = "widget/identifiers_input.html")]
pub struct IdentifiersInput {
    entries: Vec<EntryInput>,
    blank: EntryInput,
    candidates: IdentifierCandidates,
}

impl askama::filters::HtmlSafe for IdentifiersInput {}

impl IdentifiersInput {
    /// Draw the entries of `form` beside the refusals of a Save
    pub fn new(
        form: &IdentifiersForm,
        refusals: &Refusals,
        candidates: IdentifierCandidates,
    ) -> IdentifiersInput {
        let entries = form
            .identifier
            .iter()
            .enumerate()
            .map(|(n, entry)| {
                let field = format!("identifier[{n}][value]");
                EntryInput {
                    entry: entry.clone(),
                    refusal: refusals.of(&field).map(str::to_owned),
                    autofocus: false,
                    sources: proposal::sources(&candidates.candidates, &entry.proposal),
                }
            })
            .collect();
        IdentifiersInput {
            entries,
            blank: EntryInput {
                entry: IdentifierEntry {
                    kind: IdentifierKind::Isbn.to_string(),
                    ..IdentifierEntry::default()
                },
                refusal: None,
                autofocus: true,
                sources: None,
            },
            candidates,
        }
    }
}
