use std::collections::HashMap;

use crate::fuzzy::normalize;
use crate::import::Lookup;
use crate::input::{self, ContributorInput, PersonRef, WorkRef};
use crate::person::PersonRawInput;
use crate::publication::PublicationRawInput;
use crate::suggest::DraftPersonSummary;
use crate::summary::PersonSummary;
use crate::work::WorkRawInput;

#[derive(Debug)]
struct SavedDraft {
    /// The publication's own fields; its contents are the references beside them
    fields: PublicationRawInput,
    contents: Vec<WorkRef>,
    /// The API lookup outcome, if there was one
    lookup: Option<Lookup>,
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
    /// Keyed by the pending import's row id
    publications: HashMap<i64, SavedDraft>,
    /// Keyed by draft work id
    works: HashMap<i64, WorkRawInput>,
    /// Keyed by draft person id
    persons: HashMap<i64, PersonRawInput>,
}

impl DraftStore {
    pub(crate) fn snapshot(&self, library_id: i64, import_id: i64) -> Option<Snapshot> {
        let library = self.libraries.get(&library_id)?;
        let saved = library.publications.get(&import_id)?;
        let contents = saved
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
            fields: saved.fields.clone(),
            contents,
            lookup: saved.lookup.clone(),
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

    /// Store a draft publication with its works whole.
    pub(crate) fn save(
        &mut self,
        library_id: i64,
        import_id: i64,
        mut input: PublicationRawInput,
        vanished: &[i64],
        stored: &[PersonSummary],
    ) {
        let library = self.libraries.entry(library_id).or_default();
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
        let saved = library
            .publications
            .entry(import_id)
            .or_insert_with(|| SavedDraft {
                fields: PublicationRawInput::default(),
                contents: Vec::new(),
                lookup: None,
            });
        saved.fields = input;
        saved.contents = contents;
        saved.dedupe();
        library.collect();
    }

    /// Note what a source API lookup produced
    pub(crate) fn record_lookup(&mut self, library_id: i64, import_id: i64, lookup: Lookup) {
        if let Some(saved) = self
            .libraries
            .get_mut(&library_id)
            .and_then(|library| library.publications.get_mut(&import_id))
        {
            saved.lookup = Some(lookup);
        }
    }

    /// Move drafts' references from each absorbed work to its survivor
    ///
    /// Returns `(library_id, from, into)` in the order the merges happened.
    pub(crate) fn follow_merges(&mut self, merges: &[(i64, i64, i64)]) {
        for &(library_id, from, into) in merges {
            let Some(library) = self.libraries.get_mut(&library_id) else {
                continue;
            };
            for saved in library.publications.values_mut() {
                for reference in &mut saved.contents {
                    if *reference == WorkRef::Stored(from) {
                        *reference = WorkRef::Stored(into);
                    }
                }
                saved.dedupe();
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
        library.publications.remove(&import_id);
        for saved in library.publications.values_mut() {
            for reference in &mut saved.contents {
                if let WorkRef::Draft(id) = reference
                    && let Some((_, stored)) = works.iter().find(|(draft, _)| draft == id)
                {
                    *reference = WorkRef::Stored(*stored);
                }
            }
            saved.dedupe();
            for credit in &mut saved.fields.contributors {
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
                    .publications
                    .values()
                    .find(|saved| credits_draft_person(&saved.fields.contributors, *id))
                    .map(|saved| saved.fields.title.clone());
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

    pub(crate) fn forget(&mut self, library_id: i64, import_id: i64) {
        if let Some(library) = self.libraries.get_mut(&library_id) {
            library.publications.remove(&import_id);
            library.collect();
        }
    }

    pub(crate) fn drop_library(&mut self, library_id: i64) {
        self.libraries.remove(&library_id);
    }
}

impl SavedDraft {
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
            self.publications
                .values()
                .any(|saved| saved.contents.contains(&WorkRef::Draft(*id)))
        });
        self.persons.retain(|id, _| {
            self.publications
                .values()
                .any(|saved| credits_draft_person(&saved.fields.contributors, *id))
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

        store.save(1, 10, album(vec![work("Gymnopedie No. 1")]), &[], &[]);
        assert_eq!(
            contents(&store, 1, 10),
            [(WorkRef::Draft(1), "Gymnopedie No. 1".to_string())]
        );
        store.save(
            2,
            20,
            album(vec![work("Chapter One"), work("Chapter Two")]),
            &[],
            &[],
        );
        assert_eq!(
            contents(&store, 2, 20)
                .into_iter()
                .map(|(id, _)| id)
                .collect::<Vec<_>>(),
            [WorkRef::Draft(2), WorkRef::Draft(3)]
        );

        // Dropping a work does not hand its id to the next one added
        store.save(1, 10, album(vec![work("Gymnopedie No. 2")]), &[], &[]);
        assert_eq!(
            contents(&store, 1, 10),
            [(WorkRef::Draft(4), "Gymnopedie No. 2".to_string())]
        );

        store.record_lookup(1, 10, Lookup::Found);
        assert_eq!(store.snapshot(1, 10).unwrap().lookup, Some(Lookup::Found));

        store.drop_library(1);
        assert!(store.snapshot(1, 10).is_none());
        assert_eq!(contents(&store, 2, 20).len(), 2);

        store.forget(2, 20);
        assert!(store.snapshot(2, 20).is_none());
    }

    #[test]
    fn a_shared_work_is_collected_when_nothing_holds_it() {
        let mut store = DraftStore::default();
        store.save(1, 10, album(vec![work("Nocturne")]), &[], &[]);
        let nocturne = WorkRawInput {
            id: Some(WorkRef::Draft(1)),
            ..work("Nocturne")
        };
        store.save(1, 20, album(vec![nocturne]), &[], &[]);

        // Dropped from one import, kept by the other
        store.save(1, 10, album(vec![]), &[], &[]);
        assert_eq!(contents(&store, 1, 20).len(), 1);
        assert!(store.libraries[&1].works.contains_key(&1));

        // Discarded by the last import that held it, so it is collected
        store.forget(1, 20);
        assert!(store.libraries[&1].works.is_empty());
    }

    #[test]
    fn accepting_relinks_the_other_imports() {
        let mut store = DraftStore::default();
        store.save(
            1,
            10,
            album(vec![work("Nocturne"), work("Mazurka")]),
            &[],
            &[],
        );
        store.save(
            1,
            20,
            album(vec![
                WorkRawInput {
                    id: Some(WorkRef::Draft(1)),
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
        store.accept(1, 10, &[(1, 7), (2, 8)], &[]);

        assert!(store.snapshot(1, 10).is_none());
        assert_eq!(
            contents(&store, 1, 20),
            [
                (WorkRef::Stored(7), String::new()),
                (WorkRef::Draft(3), "Waltz".to_string()),
            ]
        );
        assert_eq!(
            store.libraries[&1]
                .works
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [3]
        );
    }
}
