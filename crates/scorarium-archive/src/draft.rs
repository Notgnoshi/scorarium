use std::collections::{BTreeMap, HashMap};

use crate::external_id::EntityKind;
use crate::fuzzy::normalize;
use crate::holding::{Holding, HoldingInput};
use crate::import::Lookup;
use crate::input::{self, ContributorInput, PersonRef, WorkRef};
use crate::person::{ExternalPerson, PersonRawInput};
use crate::publication::PublicationRawInput;
use crate::suggest::DraftPersonSummary;
use crate::summary::PersonSummary;
use crate::work::{WorkMerge, WorkRawInput};

#[derive(Debug, Default)]
pub(crate) struct ImportEntry {
    pub(crate) query: String,
    pub(crate) holdings: Vec<Holding>,
    pub(crate) created_at: i64,
    accepting: bool,
    saved: bool,
    /// The publication's own fields; its contents are the references beside them
    fields: PublicationRawInput,
    contents: Vec<WorkRef>,
    /// The API lookup outcome, if there was one
    lookup: Option<Lookup>,
    /// What lookups found about persons before we are able to associate them with a draft or stored person
    external_persons: HashMap<i64, ExternalPerson>,
}

#[derive(Debug)]
pub(crate) enum Content {
    Draft(Box<WorkRawInput>),
    Stored(i64),
}

/// A draft publication with its draft works filled in
#[derive(Debug)]
pub(crate) struct Snapshot {
    pub fields: PublicationRawInput,
    pub contents: Vec<Content>,
    pub lookup: Option<Lookup>,
}

#[derive(Debug, Default)]
pub(crate) struct DraftStore {
    libraries: HashMap<i64, LibraryDrafts>,
    /// The last id handed out, which is never reused.
    last_id: i64,
}

#[derive(Debug, Default)]
struct LibraryDrafts {
    /// Keyed by import id. Ids are handed out in order, so iteration is oldest first.
    imports: BTreeMap<i64, ImportEntry>,
    /// Keyed by draft work id
    works: HashMap<i64, WorkRawInput>,
    /// Keyed by draft person id
    persons: HashMap<i64, PersonRawInput>,
}

impl DraftStore {
    /// Start an import, returning its id and the holdings with ids of their own.
    pub(crate) fn start_import(
        &mut self,
        library_id: i64,
        query: &str,
        holdings: &[HoldingInput],
        created_at: i64,
    ) -> (i64, Vec<Holding>) {
        self.last_id += 1;
        let id = self.last_id;
        let holdings: Vec<Holding> = holdings
            .iter()
            .map(|holding| {
                self.last_id += 1;
                Holding {
                    id: self.last_id,
                    kind: holding.kind,
                    location: holding.location.clone(),
                }
            })
            .collect();
        let entry = ImportEntry {
            query: query.to_string(),
            holdings: holdings.clone(),
            created_at,
            ..ImportEntry::default()
        };
        self.libraries
            .entry(library_id)
            .or_default()
            .imports
            .insert(id, entry);
        (id, holdings)
    }

    pub(crate) fn imports(
        &self,
        library_id: Option<i64>,
    ) -> impl Iterator<Item = (i64, i64, &ImportEntry)> {
        self.libraries
            .iter()
            .filter(move |(id, _)| library_id.is_none_or(|wanted| wanted == **id))
            .flat_map(|(library_id, library)| {
                library
                    .imports
                    .iter()
                    .map(|(id, entry)| (*library_id, *id, entry))
            })
    }

    pub(crate) fn begin_accept(&mut self, library_id: i64, import_id: i64) -> Option<Snapshot> {
        let entry = self.editable(library_id, import_id)?;
        if !entry.saved {
            return None;
        }
        entry.accepting = true;
        self.snapshot(library_id, import_id)
    }

    pub(crate) fn end_accept(&mut self, library_id: i64, import_id: i64) {
        if let Some(entry) = self
            .libraries
            .get_mut(&library_id)
            .and_then(|library| library.imports.get_mut(&import_id))
        {
            entry.accepting = false;
        }
    }

    fn editable(&mut self, library_id: i64, import_id: i64) -> Option<&mut ImportEntry> {
        self.libraries
            .get_mut(&library_id)
            .and_then(|library| library.imports.get_mut(&import_id))
            .filter(|entry| !entry.accepting)
    }

    pub(crate) fn snapshot(&self, library_id: i64, import_id: i64) -> Option<Snapshot> {
        let library = self.libraries.get(&library_id)?;
        let entry = library
            .imports
            .get(&import_id)
            .filter(|entry| entry.saved)?;
        let contents = entry
            .contents
            .iter()
            .map(|reference| match reference {
                WorkRef::Draft(id) => Content::Draft(Box::new(
                    library
                        .works
                        .get(id)
                        .expect("a draft publication only references works the store holds")
                        .clone(),
                )),
                WorkRef::Stored(id) => Content::Stored(*id),
            })
            .collect();
        Some(Snapshot {
            fields: entry.fields.clone(),
            contents,
            lookup: entry.lookup.clone(),
        })
    }

    pub(crate) fn work(&self, library_id: i64, id: i64) -> Option<WorkRawInput> {
        self.libraries.get(&library_id)?.works.get(&id).cloned()
    }

    pub(crate) fn works(&self, library_id: i64) -> Vec<WorkRawInput> {
        self.libraries
            .get(&library_id)
            .map(|library| library.works.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Store a draft publication with its works whole. False when the import is gone or an accept
    /// has claimed it.
    pub(crate) fn save(
        &mut self,
        library_id: i64,
        import_id: i64,
        mut input: PublicationRawInput,
        vanished: &[i64],
        stored: &[PersonSummary],
    ) -> bool {
        if self.editable(library_id, import_id).is_none() {
            return false;
        }
        let library = self
            .libraries
            .get_mut(&library_id)
            .expect("checked just above");
        let mut created = HashMap::new();
        for credit in input.contributors_mut() {
            resolve_credit(&mut self.last_id, library, stored, &mut created, credit);
        }
        let mut contents = Vec::with_capacity(input.contents.len());
        for mut work in std::mem::take(&mut input.contents) {
            let reference = match work.id {
                Some(WorkRef::Stored(id)) if !vanished.contains(&id) => WorkRef::Stored(id),
                Some(WorkRef::Draft(id)) if library.works.contains_key(&id) => WorkRef::Draft(id),
                _ => {
                    self.last_id += 1;
                    WorkRef::Draft(self.last_id)
                }
            };
            if let WorkRef::Draft(id) = reference {
                work.id = Some(reference);
                library.works.insert(id, work);
            }
            contents.push(reference);
        }
        let entry = library
            .imports
            .get_mut(&import_id)
            .expect("checked just above");
        entry.saved = true;
        entry.fields = input;
        entry.contents = contents;
        entry.dedupe();
        library.collect();
        true
    }

    pub(crate) fn add_external_person(
        &mut self,
        library_id: i64,
        import_id: i64,
        mut person: ExternalPerson,
    ) -> Option<i64> {
        person.links = input::valid_links(EntityKind::Person, &person.links)
            .into_iter()
            .map(|link| link.url)
            .collect();
        let id = self.last_id + 1;
        self.editable(library_id, import_id)?
            .external_persons
            .insert(id, person);
        self.last_id = id;
        Some(id)
    }

    pub(crate) fn external_persons(
        &self,
        library_id: i64,
        import_id: i64,
    ) -> HashMap<i64, ExternalPerson> {
        self.libraries
            .get(&library_id)
            .and_then(|library| library.imports.get(&import_id))
            .map(|entry| entry.external_persons.clone())
            .unwrap_or_default()
    }

    /// Note what a source API lookup produced
    pub(crate) fn record_lookup(&mut self, library_id: i64, import_id: i64, lookup: Lookup) {
        if let Some(entry) = self.editable(library_id, import_id) {
            entry.lookup = Some(lookup);
        }
    }

    /// Move drafts' references from each absorbed work to its survivor
    pub(crate) fn relink_works(&mut self, merges: &[WorkMerge]) {
        for merge in merges {
            let Some(library) = self.libraries.get_mut(&merge.library_id) else {
                continue;
            };
            for entry in library.imports.values_mut() {
                for reference in &mut entry.contents {
                    if *reference == WorkRef::Stored(merge.from) {
                        *reference = WorkRef::Stored(merge.into);
                    }
                }
                entry.dedupe();
            }
        }
    }

    pub(crate) fn accept(
        &mut self,
        library_id: i64,
        import_id: i64,
        works: &[(i64, i64)],
        persons: &[(i64, i64)],
    ) {
        let Some(library) = self.libraries.get_mut(&library_id) else {
            return;
        };
        library.imports.remove(&import_id);
        for entry in library.imports.values_mut() {
            for reference in &mut entry.contents {
                if let WorkRef::Draft(id) = reference
                    && let Some((_, stored)) = works.iter().find(|(draft, _)| draft == id)
                {
                    *reference = WorkRef::Stored(*stored);
                }
            }
            entry.dedupe();
            for credit in &mut entry.fields.contributors {
                relink_person(credit, persons);
            }
        }
        for work in library.works.values_mut() {
            for credit in &mut work.contributors {
                relink_person(credit, persons);
            }
        }
        library.collect();
    }

    pub(crate) fn persons(&self, library_id: i64) -> HashMap<i64, PersonRawInput> {
        self.libraries
            .get(&library_id)
            .map(|library| library.persons.clone())
            .unwrap_or_default()
    }

    /// Each draft person with the title of one draft crediting them, to tell namesakes apart.
    pub(crate) fn person_summaries(&self, library_id: i64) -> Vec<DraftPersonSummary> {
        let Some(library) = self.libraries.get(&library_id) else {
            return Vec::new();
        };
        library
            .persons
            .iter()
            .map(|(id, person)| {
                let publication = library
                    .imports
                    .values()
                    .find(|entry| credits_draft_person(&entry.fields.contributors, *id))
                    .map(|entry| entry.fields.title.clone());
                let work = library
                    .works
                    .values()
                    .find(|work| credits_draft_person(&work.contributors, *id))
                    .map(|work| work.title.clone());
                DraftPersonSummary {
                    id: *id,
                    name: person.name.clone(),
                    title: publication.or(work).unwrap_or_default(),
                }
            })
            .collect()
    }

    pub(crate) fn forget(&mut self, library_id: i64, import_id: i64) -> bool {
        if self.editable(library_id, import_id).is_none() {
            return false;
        }
        let library = self
            .libraries
            .get_mut(&library_id)
            .expect("checked just above");
        library.imports.remove(&import_id);
        library.collect();
        true
    }

    pub(crate) fn drop_library(&mut self, library_id: i64) {
        self.libraries.remove(&library_id);
    }
}

impl ImportEntry {
    fn dedupe(&mut self) {
        let mut seen = Vec::with_capacity(self.contents.len());
        self.contents.retain(|reference| {
            if seen.contains(reference) {
                return false;
            }
            seen.push(*reference);
            true
        });
    }
}

impl LibraryDrafts {
    fn collect(&mut self) {
        self.works.retain(|id, _| {
            self.imports
                .values()
                .any(|entry| entry.contents.contains(&WorkRef::Draft(*id)))
        });
        self.persons.retain(|id, _| {
            self.imports
                .values()
                .any(|entry| credits_draft_person(&entry.fields.contributors, *id))
                || self
                    .works
                    .values()
                    .any(|work| credits_draft_person(&work.contributors, *id))
        });
    }
}

fn credits_draft_person(credits: &[ContributorInput], id: i64) -> bool {
    credits
        .iter()
        .any(|credit| credit.person == PersonRef::Draft(id))
}

/// Decide whom one credit refers to, creating a draft person for a new name.
fn resolve_credit(
    last_id: &mut i64,
    library: &mut LibraryDrafts,
    stored: &[PersonSummary],
    created: &mut HashMap<String, i64>,
    credit: &mut ContributorInput,
) {
    if credit.name.trim().is_empty() {
        return;
    }
    match credit.person {
        PersonRef::New => {}
        PersonRef::Linked(id) if stored.iter().any(|person| person.id == id) => return,
        PersonRef::Draft(id) if library.persons.contains_key(&id) => return,
        // A person picked on a page can be gone by the time the page is saved
        PersonRef::Linked(_) | PersonRef::Draft(_) | PersonRef::Unresolved => {
            let stored = stored
                .iter()
                .map(|person| (PersonRef::Linked(person.id), person.name.as_str()));
            let drafts = library
                .persons
                .iter()
                .map(|(id, person)| (PersonRef::Draft(*id), person.name.as_str()));
            credit.person = input::resolve_name(&credit.name, stored.chain(drafts));
        }
    }
    if credit.person == PersonRef::New {
        let id = *created.entry(normalize(&credit.name)).or_insert_with(|| {
            *last_id += 1;
            library.persons.insert(
                *last_id,
                PersonRawInput {
                    name: credit.name.trim().to_string(),
                    links: Vec::new(),
                },
            );
            *last_id
        });
        credit.person = PersonRef::Draft(id);
    }
}

fn relink_person(credit: &mut ContributorInput, persons: &[(i64, i64)]) {
    if let PersonRef::Draft(id) = credit.person
        && let Some((_, stored)) = persons.iter().find(|(draft, _)| *draft == id)
    {
        credit.person = PersonRef::Linked(*stored);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::holding::{HoldingInput, HoldingKind};

    fn work(title: &str) -> WorkRawInput {
        WorkRawInput {
            title: title.into(),
            ..WorkRawInput::default()
        }
    }

    fn album(contents: Vec<WorkRawInput>) -> PublicationRawInput {
        PublicationRawInput {
            contents,
            ..PublicationRawInput::default()
        }
    }

    fn contents(store: &DraftStore, library_id: i64, import_id: i64) -> Vec<(WorkRef, String)> {
        store
            .snapshot(library_id, import_id)
            .expect("a saved draft")
            .contents
            .into_iter()
            .map(|content| match content {
                Content::Draft(work) => (work.id.expect("a draft work carries its id"), work.title),
                Content::Stored(id) => (WorkRef::Stored(id), String::new()),
            })
            .collect()
    }

    #[test]
    fn work_ids_are_never_shared_and_a_library_takes_only_its_drafts() {
        let mut store = DraftStore::default();
        let (first, _) = store.start_import(1, "", &[], 0);
        let (second, _) = store.start_import(2, "", &[], 0);

        store.save(1, first, album(vec![work("Gymnopedie No. 1")]), &[], &[]);
        assert_eq!(
            contents(&store, 1, first),
            [(WorkRef::Draft(3), "Gymnopedie No. 1".to_string())]
        );
        store.save(
            2,
            second,
            album(vec![work("Chapter One"), work("Chapter Two")]),
            &[],
            &[],
        );
        assert_eq!(
            contents(&store, 2, second)
                .into_iter()
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
            [WorkRef::Draft(4), WorkRef::Draft(5)]
        );

        // Dropping a work does not hand its id to the next one added
        store.save(1, first, album(vec![work("Gymnopedie No. 2")]), &[], &[]);
        assert_eq!(
            contents(&store, 1, first),
            [(WorkRef::Draft(6), "Gymnopedie No. 2".to_string())]
        );

        store.record_lookup(1, first, Lookup::Found);
        assert_eq!(
            store.snapshot(1, first).unwrap().lookup,
            Some(Lookup::Found)
        );

        store.drop_library(1);
        assert!(store.snapshot(1, first).is_none());
        assert_eq!(contents(&store, 2, second).len(), 2);

        store.forget(2, second);
        assert!(store.snapshot(2, second).is_none());
    }

    #[test]
    fn a_shared_work_is_collected_when_nothing_holds_it() {
        let mut store = DraftStore::default();
        let (first, _) = store.start_import(1, "", &[], 0);
        let (second, _) = store.start_import(1, "", &[], 0);
        store.save(1, first, album(vec![work("Nocturne")]), &[], &[]);
        let nocturne = WorkRawInput {
            id: Some(WorkRef::Draft(3)),
            ..work("Nocturne")
        };
        store.save(1, second, album(vec![nocturne]), &[], &[]);

        // Dropped from one import, kept by the other
        store.save(1, first, album(vec![]), &[], &[]);
        assert_eq!(contents(&store, 1, second).len(), 1);
        assert!(store.libraries[&1].works.contains_key(&3));

        // Discarded by the last import that held it, so it is collected
        store.forget(1, second);
        assert!(store.libraries[&1].works.is_empty());
    }

    #[test]
    fn accepting_relinks_the_other_imports() {
        let mut store = DraftStore::default();
        let (first, _) = store.start_import(1, "", &[], 0);
        let (second, _) = store.start_import(1, "", &[], 0);
        store.save(
            1,
            first,
            album(vec![work("Nocturne"), work("Mazurka")]),
            &[],
            &[],
        );
        store.save(
            1,
            second,
            album(vec![
                WorkRawInput {
                    id: Some(WorkRef::Draft(3)),
                    ..work("Nocturne")
                },
                work("Waltz"),
                WorkRawInput {
                    id: Some(WorkRef::Stored(7)),
                    ..WorkRawInput::default()
                },
            ]),
            &[],
            &[],
        );

        // The catalog kept the nocturne as work 7, absorbed into the duplicate the second import
        // holds, and the mazurka as work 8. The second import then holds work 7 once.
        store.accept(1, first, &[(3, 7), (4, 8)], &[]);

        assert!(store.snapshot(1, first).is_none());
        assert_eq!(
            contents(&store, 1, second),
            [
                (WorkRef::Stored(7), String::new()),
                (WorkRef::Draft(5), "Waltz".to_string()),
            ]
        );
        assert_eq!(
            store.libraries[&1]
                .works
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [5]
        );
    }
}
