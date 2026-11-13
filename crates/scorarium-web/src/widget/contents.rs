use std::collections::HashMap;
use std::collections::hash_map::Entry;

use askama::Template;
use scorarium_engine::Engine;
use scorarium_engine::contributor::Role;
use scorarium_engine::entity::{Contributed, Entity};
use scorarium_engine::library::LibraryId;
use scorarium_engine::person::{Person, PersonId};
use scorarium_engine::publication::Publication;
use scorarium_engine::search::same_name;
use scorarium_engine::value::{Classification, NonBlank};
use scorarium_engine::work::{CatalogNumber, Work, WorkId};
use serde::Deserialize;

use crate::error::Result;
use crate::forms;
use crate::page::form_error;
use crate::page::library::entity::{self, ContainedWork, book_order, is_primary};
use crate::widget::contributors_input::{NewPeople, PICK, PersonPick};
use crate::widget::refusals::Refusals;
use crate::widget::typeahead::Typeahead;

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
struct WorkEntry {
    saved: Option<String>,
    removed: bool,
    /// An id, `new`, or blank
    person: String,
    name: String,
    role: String,
    /// An id, `new`, or blank
    work: String,
    title: String,
    catalog_number: String,
}

impl WorkEntry {
    /// Get the id of the existing work that the title picker holds, if any
    fn chosen_work(&self) -> Option<&str> {
        match self.work.trim() {
            "" | "new" => None,
            id => Some(id),
        }
    }
}

#[derive(Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ContentsForm {
    #[serde(deserialize_with = "forms::entries")]
    works: Vec<WorkEntry>,
}

pub(crate) struct NewWork {
    pub title: NonBlank,
    pub catalog_number: Option<CatalogNumber>,
    pub contributor: Option<(PersonPick, Role)>,
}

pub(crate) enum WorkPick {
    Existing(WorkId),
    New(NewWork),
}

pub(crate) struct ContentsEdits {
    pub add: Vec<WorkPick>,
    pub remove: Vec<WorkId>,
}

impl From<&[Work]> for ContentsForm {
    fn from(works: &[Work]) -> Self {
        let mut contained: Vec<ContainedWork> = works.iter().map(Into::into).collect();
        book_order(&mut contained);
        let works = contained
            .iter()
            .map(|work| WorkEntry {
                saved: Some(work.id.to_string()),
                ..WorkEntry::default()
            })
            .collect();
        ContentsForm { works }
    }
}

/// Find the work among `works` that carries `number`, the lowest id breaking ties
fn matching_work(works: &[Work], number: &CatalogNumber) -> Option<WorkId> {
    works
        .iter()
        .filter(|work| work.catalog_numbers().contains(number))
        .map(Work::id)
        .min_by_key(|id| id.get())
}

fn find<'a>(works: &'a [Work], saved: &str) -> Option<&'a Work> {
    works.iter().find(|work| work.id().to_string() == saved)
}

impl ContentsForm {
    /// Parse the entries, dropping each one whose chosen work is gone from the library
    pub async fn parse(
        &mut self,
        engine: &Engine,
        publication: &Publication,
        refusals: &mut Refusals,
    ) -> Result<ContentsEdits> {
        let library = publication.library();
        let mut kept = Vec::with_capacity(self.works.len());
        let mut chosen = Vec::with_capacity(self.works.len());
        for entry in std::mem::take(&mut self.works) {
            let work = match entry.chosen_work() {
                Some(id) => match entity::in_library::<Work>(engine, library, id, "work").await {
                    Ok(work) => Some(work.id()),
                    Err(e) => {
                        let (_, message) = form_error(e)?;
                        refusals.refuse(message);
                        continue;
                    }
                },
                None => None,
            };
            kept.push(entry);
            chosen.push(work);
        }
        self.works = kept;

        let stored = publication.works().await?;
        let mut remove = Vec::new();
        let mut add = Vec::new();
        let mut works_of: HashMap<PersonId, Vec<Work>> = HashMap::new();
        for (n, (entry, chosen)) in self.works.iter().zip(chosen).enumerate() {
            if let Some(saved) = &entry.saved {
                if entry.removed
                    && let Some(work) = find(&stored, saved)
                {
                    remove.push(work.id());
                }
                continue;
            }
            if entry.title.trim().is_empty() && entry.catalog_number.trim().is_empty() {
                continue;
            }
            if let Some(work) = chosen {
                add.push((n, WorkPick::Existing(work)));
                continue;
            }
            let field = |name: &str| format!("works[{n}][{name}]");
            let title = refusals.parse::<NonBlank>(&field("title"), &entry.title);
            let number = match entry.catalog_number.trim() {
                "" => Some(None),
                number => refusals
                    .parse::<CatalogNumber>(&field("catalog_number"), number)
                    .map(Some),
            };
            let contributor = if entry.person.trim().is_empty() && entry.name.trim().is_empty() {
                Some(None)
            } else {
                let pick = match entry.person.trim() {
                    "" => {
                        refusals.refuse_field(&field("name"), PICK);
                        None
                    }
                    "new" => refusals
                        .parse(&field("name"), &entry.name)
                        .map(PersonPick::New),
                    id => {
                        let person = refusals.check_field(
                            &field("name"),
                            entity::in_library::<Person>(engine, library, id, "person").await,
                        );
                        if let Some(person) = &person
                            && let Some(Some(_)) = number
                            && let Entry::Vacant(works) = works_of.entry(person.id())
                        {
                            works.insert(person.works().await?);
                        }
                        person.map(|person| PersonPick::Existing(person.id()))
                    }
                };
                let role = refusals.parse::<Role>(&field("role"), &entry.role);
                pick.zip(role).map(Some)
            };
            let (Some(title), Some(catalog_number), Some(contributor)) =
                (title, number, contributor)
            else {
                continue;
            };
            let matched = match (&contributor, &catalog_number) {
                (Some((PersonPick::Existing(person), _)), Some(number)) => works_of
                    .get(person)
                    .and_then(|works| matching_work(works, number)),
                _ => None,
            };
            let pick = match matched {
                Some(work) => WorkPick::Existing(work),
                None => WorkPick::New(NewWork {
                    title,
                    catalog_number,
                    contributor,
                }),
            };
            add.push((n, pick));
        }

        for (n, pick) in &add {
            let stays = stored
                .iter()
                .map(Work::id)
                .filter(|id| !remove.contains(id))
                .any(|id| pick.is_same_work(&WorkPick::Existing(id)));
            let repeated = add
                .iter()
                .any(|(other, added)| other != n && pick.is_same_work(added));
            if stays || repeated {
                refusals.refuse_field(&format!("works[{n}][title]"), "Duplicate");
            }
        }

        let add: Vec<WorkPick> = add.into_iter().map(|(_, pick)| pick).collect();
        remove.retain(|id| {
            !add.iter()
                .any(|pick| matches!(pick, WorkPick::Existing(added) if added == id))
        });
        Ok(ContentsEdits { add, remove })
    }
}

impl WorkPick {
    /// Add the work to `publication`, creating it if it is new
    pub async fn add_to(
        self,
        publication: &mut Publication,
        people: &mut NewPeople,
    ) -> scorarium_engine::Result<()> {
        match self {
            WorkPick::Existing(work) => publication.add_work(work).await,
            WorkPick::New(new) => {
                let mut work = publication.create_work(new.title).await?;
                if let Some(number) = new.catalog_number {
                    work.add_catalog_number(number).await?;
                }
                if let Some((pick, role)) = new.contributor {
                    people.attach(&mut work, pick, role).await?;
                }
                Ok(())
            }
        }
    }

    /// Check whether both picks add the same work, which a new work without both a person and a
    /// catalog number never is
    fn is_same_work(&self, other: &WorkPick) -> bool {
        match (self, other) {
            (WorkPick::Existing(one), WorkPick::Existing(other)) => one == other,
            (WorkPick::New(one), WorkPick::New(other)) => {
                let (Some(number), Some((person, _))) = (&one.catalog_number, &one.contributor)
                else {
                    return false;
                };
                other.catalog_number.as_ref() == Some(number)
                    && other
                        .contributor
                        .as_ref()
                        .is_some_and(|(other, _)| is_same_person(person, other))
            }
            _ => false,
        }
    }
}

fn is_same_person(one: &PersonPick, other: &PersonPick) -> bool {
    match (one, other) {
        (PersonPick::Existing(one), PersonPick::Existing(other)) => one == other,
        (PersonPick::New(one), PersonPick::New(other)) => same_name(one.as_str(), other.as_str()),
        _ => false,
    }
}

enum EntryInput {
    Saved {
        work: ContainedWork,
        removed: bool,
    },
    New {
        person: Box<Typeahead>,
        role: Box<Typeahead>,
        title: Box<Typeahead>,
        catalog_number: String,
        /// Why a Save refused the catalog number
        refusal: Option<String>,
        /// Whether the title is chosen, which disables the other fields
        locked: bool,
    },
}

#[derive(Template)]
#[template(path = "widget/contents.html")]
pub struct Contents {
    library: LibraryId,
    /// The page that the pencil of a work returns to
    back: String,
    entries: Vec<(String, EntryInput)>,
    blank: EntryInput,
}

impl askama::filters::HtmlSafe for Contents {}

/// Draw an entry that is not saved, with the fields of `entry`
fn new_entry(
    library: LibraryId,
    key: &str,
    entry: &WorkEntry,
    locked: bool,
    refusals: &Refusals,
) -> EntryInput {
    let field = |field: &str, suggests: &str, placeholder: &str, value: &str| {
        let name = format!("works[{key}][{field}]");
        Typeahead {
            id: format!("works-{key}-{field}"),
            refusal: refusals.of(&name).map(str::to_owned),
            name,
            url: format!("/api/library/{library}/suggest/{suggests}"),
            placeholder: placeholder.to_owned(),
            value: value.to_owned(),
            picked: None,
            pick_required: None,
            attrs: vec![],
        }
    };
    let mut person = Typeahead {
        picked: Some((format!("works[{key}][person]"), entry.person.clone())),
        pick_required: Some(PICK.to_owned()),
        ..field("name", "person", "Person", &entry.name)
    };
    let mut role = field("role", "role", "Role", &entry.role);
    let title = Typeahead {
        picked: Some((format!("works[{key}][work]"), entry.work.clone())),
        attrs: vec![
            ("data-typeahead-with", format!("works[{key}][person]")),
            // A typed title is more often a new work than an existing one.
            ("data-no-default-pick", String::new()),
        ],
        ..field("title", "work", "Title", &entry.title)
    };
    if locked {
        person.attrs.push(("disabled", String::new()));
        role.attrs.push(("disabled", String::new()));
    }
    let number = format!("works[{key}][catalog_number]");
    EntryInput::New {
        person: Box::new(person),
        role: Box::new(role),
        title: Box::new(title),
        catalog_number: entry.catalog_number.clone(),
        refusal: refusals.of(&number).map(str::to_owned),
        locked,
    }
}

impl Contents {
    /// Draw the entries of `form` beside the stored works and the refusals of a Save
    pub async fn of(
        engine: &Engine,
        publication: &Publication,
        form: &ContentsForm,
        back: &str,
        refusals: &Refusals,
    ) -> Result<Contents> {
        let library = publication.library();
        let stored = publication.works().await?;
        let mut entries = Vec::with_capacity(form.works.len());
        for (n, entry) in form.works.iter().enumerate() {
            let key = n.to_string();
            if let Some(saved) = &entry.saved {
                if let Some(work) = find(&stored, saved) {
                    let work = ContainedWork::from(work);
                    let removed = entry.removed;
                    entries.push((key, EntryInput::Saved { work, removed }));
                }
                continue;
            }
            let input = match entry.chosen_work() {
                Some(id) => match entity::in_library::<Work>(engine, library, id, "work").await {
                    Ok(work) => {
                        let work = ContainedWork::from(&work);
                        let (person, name, role) = match work.first_contributor() {
                            Some((contributor, role)) => (
                                contributor.person.to_string(),
                                contributor.name.clone(),
                                role.to_string(),
                            ),
                            None => Default::default(),
                        };
                        let chosen = WorkEntry {
                            person,
                            name,
                            role,
                            work: work.id.to_string(),
                            title: work.title,
                            catalog_number: work
                                .catalog_number
                                .map(|number| number.to_string())
                                .unwrap_or_default(),
                            ..WorkEntry::default()
                        };
                        new_entry(library, &key, &chosen, true, refusals)
                    }
                    Err(_) => {
                        let typed = WorkEntry {
                            work: String::new(),
                            ..entry.clone()
                        };
                        new_entry(library, &key, &typed, false, refusals)
                    }
                },
                None => new_entry(library, &key, entry, false, refusals),
            };
            entries.push((key, input));
        }

        let (person, name, role) = prefill(publication, &stored);
        let mut blank = new_entry(
            library,
            "__key__",
            &WorkEntry {
                person,
                name,
                role,
                ..WorkEntry::default()
            },
            false,
            refusals,
        );
        if let EntryInput::New { person, title, .. } = &mut blank {
            person.attrs.push(("autofocus", String::new()));
            title.attrs.push(("autofocus", String::new()));
        }
        Ok(Contents {
            library,
            back: back.to_owned(),
            entries,
            blank,
        })
    }
}

/// Pre-fill the sole composer or author across the publication and its works, if there is one
///
/// Return the person's id, their name, and the role, with the id and name blank and the
/// classification's conventional role when there is no sole one.
fn prefill(publication: &Publication, works: &[Work]) -> (String, String, String) {
    let primary: Vec<(PersonId, &str, &str)> = publication
        .contributors()
        .iter()
        .chain(works.iter().flat_map(|work| work.contributors()))
        .filter(|c| is_primary(&c.role))
        .map(|c| (c.person, c.name.as_str(), c.role.as_str()))
        .collect();
    match primary.first() {
        Some(&(person, name, role)) if primary.iter().all(|(p, _, _)| *p == person) => {
            (person.to_string(), name.to_owned(), role.to_owned())
        }
        _ => {
            let role = match publication.classification() {
                Some(Classification::Music) => "composer",
                _ => "author",
            };
            (String::new(), String::new(), role.to_owned())
        }
    }
}
