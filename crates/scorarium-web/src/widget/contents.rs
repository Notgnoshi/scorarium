use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt;

use askama::Template;
use scorarium_engine::Engine;
use scorarium_engine::contributor::Role;
use scorarium_engine::entity::{Contributed, Entity};
use scorarium_engine::library::LibraryId;
use scorarium_engine::person::{Person, PersonId};
use scorarium_engine::publication::{Publication, PublicationId};
use scorarium_engine::search::same_name;
use scorarium_engine::value::{Classification, NonBlank};
use scorarium_engine::work::{CatalogNumber, Work, WorkId};
use serde::Deserialize;

use crate::error::Result;
use crate::forms;
use crate::page::form_error;
use crate::page::library::entity::{self, ContainedWork, book_order, is_primary};
use crate::widget::confirm::count;
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

#[derive(Default, Deserialize)]
#[serde(default)]
pub(crate) struct SequenceForm {
    /// An id, `new`, or blank
    person: String,
    name: String,
    role: String,
    title: String,
    prefix: String,
    from: String,
    to: String,
}

pub(crate) struct Sequence {
    pub added: usize,
    pub skipped: usize,
}

impl fmt::Display for Sequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let added = count(self.added, "work");
        match self.skipped {
            0 => write!(f, "Added {added}."),
            skipped => write!(f, "Added {added}, skipped {skipped}."),
        }
    }
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
fn matching_work<'a>(works: &'a [Work], number: &CatalogNumber) -> Option<&'a Work> {
    works
        .iter()
        .filter(|work| work.catalog_numbers().contains(number))
        .min_by_key(|work| work.id().get())
}

/// Read a person picker, refusing it against `field`, and keep the works of an existing person
async fn pick_person(
    engine: &Engine,
    library: LibraryId,
    person: &str,
    name: &str,
    field: &str,
    works_of: &mut HashMap<PersonId, Vec<Work>>,
    refusals: &mut Refusals,
) -> Result<Option<PersonPick>> {
    Ok(match person.trim() {
        "" => {
            refusals.refuse_field(field, PICK);
            None
        }
        "new" => refusals.parse(field, name).map(PersonPick::New),
        id => {
            let person = refusals.check_field(
                field,
                entity::in_library::<Person>(engine, library, id, "person").await,
            );
            if let Some(person) = &person
                && let Entry::Vacant(works) = works_of.entry(person.id())
            {
                works.insert(person.works().await?);
            }
            person.map(|person| PersonPick::Existing(person.id()))
        }
    })
}

fn find<'a>(works: &'a [Work], saved: &str) -> Option<&'a Work> {
    works.iter().find(|work| work.id().to_string() == saved)
}

/// Join a catalog prefix and a number, directly after a separator and with a space otherwise
///
/// "BWV" gives "BWV 846", and "Sz. 107/" gives "Sz. 107/1".
fn catalog_number(prefix: &str, n: u32) -> String {
    let prefix = prefix.trim();
    if prefix.ends_with(['/', '-', ':']) {
        format!("{prefix}{n}")
    } else {
        format!("{prefix} {n}")
    }
}

/// Get the id, name, and role of the first person of `work`, blank when it has none
fn first_person(work: &ContainedWork) -> (String, String, String) {
    match work.first_contributor() {
        Some((contributor, role)) => (
            contributor.person.to_string(),
            contributor.name.clone(),
            role.to_string(),
        ),
        None => Default::default(),
    }
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
                let pick = pick_person(
                    engine,
                    library,
                    &entry.person,
                    &entry.name,
                    &field("name"),
                    &mut works_of,
                    refusals,
                )
                .await?;
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
                Some(work) => WorkPick::Existing(work.id()),
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

    /// Append an entry for each number of `sequence` that the list lacks
    pub async fn append(
        &mut self,
        engine: &Engine,
        publication: &Publication,
        sequence: &SequenceForm,
        refusals: &mut Refusals,
    ) -> Result<Option<Sequence>> {
        let library = publication.library();
        let title = refusals.parse::<NonBlank>("title", &sequence.title);
        let prefix = refusals.parse::<NonBlank>("prefix", &sequence.prefix);
        let role = refusals.parse::<Role>("role", &sequence.role);
        let mut works_of = HashMap::new();
        let person = pick_person(
            engine,
            library,
            &sequence.person,
            &sequence.name,
            "name",
            &mut works_of,
            refusals,
        )
        .await?;
        let range = match (
            sequence.from.trim().parse::<u32>(),
            sequence.to.trim().parse::<u32>(),
        ) {
            (Ok(from), Ok(to)) if from > to => {
                refusals.refuse_field("numbers", "From is greater than To.");
                None
            }
            (Ok(from), Ok(to)) if to - from >= 200 => {
                refusals.refuse_field("numbers", "At most 200 works.");
                None
            }
            (Ok(from), Ok(to)) => Some(from..=to),
            _ => {
                refusals.refuse_field("numbers", "Not a number.");
                None
            }
        };
        let (Some(title), Some(prefix), Some(role), Some(person), Some(range)) =
            (title, prefix, role, person, range)
        else {
            return Ok(None);
        };
        let works: &[Work] = match &person {
            PersonPick::Existing(id) => works_of.get(id).map_or(&[], Vec::as_slice),
            PersonPick::New(_) => &[],
        };
        let mut numbers = Vec::new();
        for n in range {
            let joined = catalog_number(prefix.as_str(), n);
            let Some(number) = refusals.parse::<CatalogNumber>("prefix", &joined) else {
                return Ok(None);
            };
            numbers.push((joined, number));
        }

        let edits = self
            .clone()
            .parse(engine, publication, &mut Refusals::default())
            .await?;
        let stays: Vec<WorkPick> = publication
            .works()
            .await?
            .iter()
            .map(Work::id)
            .filter(|id| !edits.remove.contains(id))
            .map(WorkPick::Existing)
            .collect();
        let mut listed = edits.add;
        let mut added = 0;
        let mut skipped = 0;
        for (joined, number) in numbers {
            let (pick, entry) = match matching_work(works, &number) {
                Some(work) => (
                    WorkPick::Existing(work.id()),
                    WorkEntry {
                        work: work.id().to_string(),
                        title: work.title().to_owned(),
                        ..WorkEntry::default()
                    },
                ),
                None => (
                    WorkPick::New(NewWork {
                        title: title.clone(),
                        catalog_number: Some(number),
                        contributor: Some((person.clone(), role.clone())),
                    }),
                    WorkEntry {
                        person: sequence.person.clone(),
                        name: sequence.name.clone(),
                        role: sequence.role.clone(),
                        title: sequence.title.clone(),
                        catalog_number: joined,
                        ..WorkEntry::default()
                    },
                ),
            };
            if stays
                .iter()
                .chain(&listed)
                .any(|other| pick.is_same_work(other))
            {
                skipped += 1;
                continue;
            }
            self.works.push(entry);
            listed.push(pick);
            added += 1;
        }
        if added == 0 {
            let message = match skipped {
                1 => "Already listed.".to_owned(),
                _ => format!("All {skipped} are already listed."),
            };
            refusals.refuse_field("numbers", message);
            return Ok(None);
        }
        Ok(Some(Sequence { added, skipped }))
    }

    /// Pre-fill the dialog from the last entry, or else from the sole composer or author
    pub async fn prefill(
        &self,
        engine: &Engine,
        publication: &Publication,
        works: &[Work],
    ) -> Result<SequenceForm> {
        let (person, name, role) = match self.works.last() {
            Some(entry) if entry.saved.is_none() => {
                let chosen = match entry.chosen_work() {
                    Some(id) => {
                        entity::in_library::<Work>(engine, publication.library(), id, "work")
                            .await
                            .ok()
                    }
                    None => None,
                };
                match chosen {
                    Some(work) => first_person(&ContainedWork::from(&work)),
                    None => (entry.person.clone(), entry.name.clone(), entry.role.clone()),
                }
            }
            _ => sole_primary(publication, works),
        };
        Ok(SequenceForm {
            person,
            name,
            role,
            ..SequenceForm::default()
        })
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
    publication: PublicationId,
    /// The page that the pencil of a work returns to
    back: String,
    entries: Vec<(String, EntryInput)>,
    blank: EntryInput,
    sequence: Option<Sequence>,
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
                        let (person, name, role) = first_person(&work);
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

        let (person, name, role) = sole_primary(publication, &stored);
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
            publication: publication.id(),
            back: back.to_owned(),
            entries,
            blank,
            sequence: None,
        })
    }

    /// Report `sequence` and put the cursor in the first entry that it added
    pub fn with_sequence(mut self, sequence: Sequence) -> Contents {
        let first = self.entries.len() - sequence.added;
        if let Some((_, EntryInput::New { title, .. })) = self.entries.get_mut(first) {
            title.attrs.push(("autofocus", String::new()));
        }
        self.sequence = Some(sequence);
        self
    }
}

/// Find the sole composer or author across the publication and its works, if there is one
///
/// Return the person's id, their name, and the role, with the id and name blank and the
/// classification's conventional role when there is no sole one.
fn sole_primary(publication: &Publication, works: &[Work]) -> (String, String, String) {
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

#[derive(Template)]
#[template(path = "widget/sequence_dialog.html")]
pub(crate) struct SequenceDialog {
    /// The route that Add posts to
    url: String,
    person: Typeahead,
    role: Typeahead,
    form: SequenceForm,
    refusals: Refusals,
}

impl SequenceDialog {
    /// Draw the dialog with the values of `form` and the refusals of an Add
    pub fn new(
        publication: &Publication,
        form: SequenceForm,
        refusals: Refusals,
    ) -> SequenceDialog {
        let library = publication.library();
        let field = |field: &str, suggests: &str, placeholder: &str, value: &str| Typeahead {
            id: format!("sequence-{field}"),
            name: field.to_owned(),
            url: format!("/api/library/{library}/suggest/{suggests}"),
            placeholder: placeholder.to_owned(),
            value: value.to_owned(),
            picked: None,
            pick_required: None,
            refusal: refusals.of(field).map(str::to_owned),
            attrs: vec![("required", String::new())],
        };
        let mut person = Typeahead {
            picked: Some(("person".to_owned(), form.person.clone())),
            pick_required: Some(PICK.to_owned()),
            ..field("name", "person", "Person", &form.name)
        };
        if form.person.trim().is_empty() {
            person.attrs.push(("autofocus", String::new()));
        }
        let role = field("role", "role", "Role", &form.role);
        SequenceDialog {
            url: format!(
                "/api/library/{library}/publication/{}/contents/sequence",
                publication.id()
            ),
            person,
            role,
            form,
            refusals,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_number_joins_a_prefix_and_a_number() {
        assert_eq!(catalog_number("BWV", 846), "BWV 846");
        assert_eq!(catalog_number("Sz. 107/", 1), "Sz. 107/1");
    }
}
