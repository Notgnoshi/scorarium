use askama::Template;
use scorarium_engine::Engine;
use scorarium_engine::contributor::{Contributor, Role};
use scorarium_engine::entity::{Contributed, Entity};
use scorarium_engine::library::LibraryId;
use scorarium_engine::person::{Person, PersonId};
use scorarium_engine::search::same_name;
use scorarium_engine::value::NonBlank;
use serde::Deserialize;

use crate::error::Result;
use crate::forms;
use crate::page::form_error;
use crate::page::library::entity;
use crate::widget::refusals::Refusals;
use crate::widget::typeahead::Typeahead;

const PICK: &str = "Pick a person from the list";

#[derive(Clone, Deserialize)]
struct SavedContributor {
    person: String,
    role: String,
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct ContributorEntry {
    saved: Option<SavedContributor>,
    removed: bool,
    /// The picked person's id, `new`, or blank
    person: String,
    /// The text of the picker
    name: String,
    role: String,
}

impl ContributorEntry {
    fn is_blank(&self) -> bool {
        self.name.trim().is_empty() && self.role.trim().is_empty()
    }

    /// Get the id of the existing person that Save adds, if any
    fn adds_existing(&self) -> Option<&str> {
        match &self.saved {
            Some(saved) if !self.removed && self.role.trim() != saved.role => Some(&saved.person),
            Some(_) => None,
            None if self.is_blank() => None,
            None => match self.person.trim() {
                "" | "new" => None,
                id => Some(id),
            },
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct ContributorsForm {
    #[serde(deserialize_with = "forms::entries")]
    contributor: Vec<ContributorEntry>,
}

pub(crate) enum PersonPick {
    Existing(PersonId),
    New(NonBlank),
}

pub(crate) struct ContributorEdits {
    pub add: Vec<(PersonPick, Role)>,
    pub remove: Vec<(PersonId, Role)>,
}

impl ContributorsForm {
    /// Parse the entries, dropping each one whose picked person is gone from `library`
    pub async fn parse(
        &mut self,
        engine: &Engine,
        library: LibraryId,
        refusals: &mut Refusals,
    ) -> Result<ContributorEdits> {
        let mut kept = Vec::with_capacity(self.contributor.len());
        for entry in std::mem::take(&mut self.contributor) {
            if let Some(id) = entry.adds_existing()
                && let Err(e) = entity::in_library::<Person>(engine, library, id, "person").await
            {
                let (_, message) = form_error(e)?;
                refusals.refuse(message);
                continue;
            }
            kept.push(entry);
        }
        self.contributor = kept;

        let mut edits = ContributorEdits {
            add: vec![],
            remove: vec![],
        };
        for (n, entry) in self.contributor.iter().enumerate() {
            let role_field = format!("contributor[{n}][role]");
            if let Some(saved) = &entry.saved {
                let (Some(person), Some(saved_role)) = (
                    refusals.check(person_id(&saved.person)),
                    refusals.check(saved.role.parse::<Role>()),
                ) else {
                    continue;
                };
                if entry.removed {
                    edits.remove.push((person, saved_role));
                } else if let Some(role) = refusals.parse::<Role>(&role_field, &entry.role)
                    && role != saved_role
                {
                    edits.add.push((PersonPick::Existing(person), role));
                    edits.remove.push((person, saved_role));
                }
                continue;
            }
            if entry.is_blank() {
                continue;
            }
            let name_field = format!("contributor[{n}][name]");
            let pick = match entry.person.trim() {
                "" => {
                    refusals.refuse_field(&name_field, PICK);
                    None
                }
                "new" => refusals
                    .parse(&name_field, &entry.name)
                    .map(PersonPick::New),
                id => refusals.check(person_id(id)).map(PersonPick::Existing),
            };
            let role = refusals.parse(&role_field, &entry.role);
            if let (Some(pick), Some(role)) = (pick, role) {
                edits.add.push((pick, role));
            }
        }

        let add = &edits.add;
        edits.remove.retain(|(person, role)| {
            !add.iter().any(|(pick, added)| {
                matches!(pick, PersonPick::Existing(id) if id == person) && added == role
            })
        });
        Ok(edits)
    }
}

impl From<&[Contributor]> for ContributorsForm {
    fn from(contributors: &[Contributor]) -> Self {
        let contributor = contributors
            .iter()
            .map(|contributor| ContributorEntry {
                saved: Some(SavedContributor {
                    person: contributor.person.to_string(),
                    role: contributor.role.to_string(),
                }),
                role: contributor.role.to_string(),
                ..ContributorEntry::default()
            })
            .collect();
        ContributorsForm { contributor }
    }
}

fn person_id(id: &str) -> scorarium_engine::Result<PersonId> {
    id.parse()
        .map_err(|_| scorarium_engine::Error::InvalidValue(format!("Unknown person '{id}'.")))
}

/// The people that a Save created, by the spelling that created them
#[derive(Default)]
pub(crate) struct NewPeople(Vec<(String, PersonId)>);

impl NewPeople {
    /// Add `pick` to `entity` in `role`, creating each new name once
    pub async fn attach<E: Contributed>(
        &mut self,
        entity: &mut E,
        pick: PersonPick,
        role: Role,
    ) -> scorarium_engine::Result<()> {
        let person = match pick {
            PersonPick::Existing(person) => person,
            PersonPick::New(name) => {
                let known = self
                    .0
                    .iter()
                    .find(|(spelling, _)| same_name(spelling, name.as_str()))
                    .map(|(_, person)| *person);
                match known {
                    Some(person) => person,
                    None => {
                        let spelling = name.as_str().to_owned();
                        let person = entity.create_contributor(name, role).await?.id();
                        self.0.push((spelling, person));
                        return Ok(());
                    }
                }
            }
        };
        entity.add_contributor(person, role).await
    }
}

struct EntryInput {
    key: String,
    entry: ContributorEntry,
    /// The name of a saved contributor
    name: String,
    /// The picker of an entry that is not saved
    person: Option<Typeahead>,
    role: Typeahead,
}

impl EntryInput {
    fn new(
        key: String,
        entry: ContributorEntry,
        name: String,
        library: LibraryId,
        refusals: &Refusals,
    ) -> EntryInput {
        let field = |field: &str| {
            let name = format!("contributor[{key}][{field}]");
            Typeahead {
                id: format!("contributor-{key}-{field}"),
                refusal: refusals.of(&name).map(str::to_owned),
                name,
                url: String::new(),
                placeholder: String::new(),
                value: String::new(),
                picked: None,
                pick_required: None,
                attrs: vec![],
            }
        };
        let person = entry.saved.is_none().then(|| Typeahead {
            url: format!("/api/library/{library}/suggest/person"),
            placeholder: "Person".to_owned(),
            value: entry.name.clone(),
            picked: Some((format!("contributor[{key}][person]"), entry.person.clone())),
            pick_required: Some(PICK.to_owned()),
            ..field("name")
        });
        let role = Typeahead {
            url: format!("/api/library/{library}/suggest/role"),
            placeholder: "Role".to_owned(),
            value: entry.role.clone(),
            ..field("role")
        };
        EntryInput {
            key,
            entry,
            name,
            person,
            role,
        }
    }
}

#[derive(Template)]
#[template(path = "widget/contributors_input.html")]
pub struct ContributorsInput {
    entries: Vec<EntryInput>,
    blank: EntryInput,
    library: LibraryId,
    /// The page that the pencil of a person returns to
    back: String,
}

impl askama::filters::HtmlSafe for ContributorsInput {}

impl ContributorsInput {
    /// Draw the entries of `form` beside the stored `contributors` and the refusals of a Save
    pub fn new(
        form: &ContributorsForm,
        contributors: &[Contributor],
        library: LibraryId,
        back: &str,
        refusals: &Refusals,
    ) -> ContributorsInput {
        let entries = form
            .contributor
            .iter()
            .enumerate()
            .filter_map(|(n, entry)| {
                let name = match &entry.saved {
                    Some(saved) => contributors
                        .iter()
                        .find(|c| c.person.to_string() == saved.person)?
                        .name
                        .clone(),
                    None => String::new(),
                };
                Some(EntryInput::new(
                    n.to_string(),
                    entry.clone(),
                    name,
                    library,
                    refusals,
                ))
            })
            .collect();
        ContributorsInput {
            entries,
            blank: EntryInput::new(
                "__key__".to_owned(),
                ContributorEntry::default(),
                String::new(),
                library,
                refusals,
            ),
            library,
            back: back.to_owned(),
        }
    }
}
