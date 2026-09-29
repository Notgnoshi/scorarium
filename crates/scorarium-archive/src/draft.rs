use std::collections::HashMap;

use crate::import::Lookup;
use crate::input::WorkRef;
use crate::publication::PublicationRawInput;
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

    /// Store a draft publication with its works whole.
    pub(crate) fn save(
        &mut self,
        library_id: i64,
        import_id: i64,
        mut input: PublicationRawInput,
        vanished: &[i64],
    ) {
        let library = self.libraries.entry(library_id).or_default();
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
        let saved = self
            .libraries
            .get_mut(&library_id)
            .and_then(|library| library.publications.get_mut(&import_id))
            .expect("a lookup is recorded on a saved draft");
        saved.lookup = Some(lookup);
    }

    pub(crate) fn accept(&mut self, library_id: i64, import_id: i64, published: &[(i64, i64)]) {
        let Some(library) = self.libraries.get_mut(&library_id) else {
            return;
        };
        library.publications.remove(&import_id);
        for saved in library.publications.values_mut() {
            for reference in &mut saved.contents {
                if let WorkRef::Draft(id) = reference
                    && let Some((_, stored)) = published.iter().find(|(draft, _)| draft == id)
                {
                    *reference = WorkRef::Stored(*stored);
                }
            }
            saved.dedupe();
        }
        library.collect();
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
    /// Remove the draft works no draft publication contains.
    fn collect(&mut self) {
        self.works.retain(|id, _| {
            self.publications
                .values()
                .any(|saved| saved.contents.contains(&WorkRef::Draft(*id)))
        });
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

        store.save(1, 10, album(vec![work("Gymnopedie No. 1")]), &[]);
        assert_eq!(
            contents(&store, 1, 10),
            [(WorkRef::Draft(1), "Gymnopedie No. 1".to_string())]
        );
        store.save(
            2,
            20,
            album(vec![work("Chapter One"), work("Chapter Two")]),
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
        store.save(1, 10, album(vec![work("Gymnopedie No. 2")]), &[]);
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
        store.save(1, 10, album(vec![work("Nocturne")]), &[]);
        let nocturne = WorkRawInput {
            id: Some(WorkRef::Draft(1)),
            ..work("Nocturne")
        };
        store.save(1, 20, album(vec![nocturne]), &[]);

        // Dropped from one import, kept by the other
        store.save(1, 10, album(vec![]), &[]);
        assert_eq!(contents(&store, 1, 20).len(), 1);
        assert!(store.libraries[&1].works.contains_key(&1));

        // Discarded by the last import that held it, so it is collected
        store.forget(1, 20);
        assert!(store.libraries[&1].works.is_empty());
    }

    #[test]
    fn accepting_relinks_the_other_imports() {
        let mut store = DraftStore::default();
        store.save(1, 10, album(vec![work("Nocturne"), work("Mazurka")]), &[]);
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
        );

        // The catalog kept the nocturne as work 7, absorbed into the duplicate the second import
        // holds, and the mazurka as work 8. The second import then holds work 7 once.
        store.accept(1, 10, &[(1, 7), (2, 8)]);

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
