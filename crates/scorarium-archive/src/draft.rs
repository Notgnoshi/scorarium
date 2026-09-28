use std::collections::HashMap;

use crate::import::Lookup;
use crate::input::WorkRef;
use crate::publication::PublicationRawInput;

#[derive(Debug)]
pub(crate) struct SavedDraft {
    pub(crate) input: PublicationRawInput,
    /// The API lookup outcome, if there was one
    pub(crate) lookup: Option<Lookup>,
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
}

impl DraftStore {
    pub(crate) fn get(&self, library_id: i64, import_id: i64) -> Option<&SavedDraft> {
        self.libraries
            .get(&library_id)?
            .publications
            .get(&import_id)
    }

    /// Store the review page's edits
    pub(crate) fn save(
        &mut self,
        library_id: i64,
        import_id: i64,
        mut input: PublicationRawInput,
    ) -> &SavedDraft {
        for work in &mut input.contents {
            if work.id.is_none() {
                self.last_id += 1;
                work.id = Some(WorkRef::Draft(self.last_id));
            }
        }
        let library = self.libraries.entry(library_id).or_default();
        let saved = library
            .publications
            .entry(import_id)
            .or_insert_with(|| SavedDraft {
                input: PublicationRawInput::default(),
                lookup: None,
            });
        saved.input = input;
        saved
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

    pub(crate) fn forget(&mut self, library_id: i64, import_id: i64) {
        if let Some(library) = self.libraries.get_mut(&library_id) {
            library.publications.remove(&import_id);
        }
    }

    pub(crate) fn drop_library(&mut self, library_id: i64) {
        self.libraries.remove(&library_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work::WorkRawInput;

    fn album(works: &[&str]) -> PublicationRawInput {
        PublicationRawInput {
            contents: works
                .iter()
                .map(|title| WorkRawInput {
                    title: (*title).into(),
                    ..WorkRawInput::default()
                })
                .collect(),
            ..PublicationRawInput::default()
        }
    }

    fn work_ids(saved: &SavedDraft) -> Vec<Option<WorkRef>> {
        saved.input.contents.iter().map(|work| work.id).collect()
    }

    #[test]
    fn work_ids_are_never_shared_and_a_library_takes_only_its_drafts() {
        let mut store = DraftStore::default();

        let scores = store.save(1, 10, album(&["Gymnopedie No. 1"]));
        assert_eq!(work_ids(scores), [Some(WorkRef::Draft(1))]);
        let books = store.save(2, 20, album(&["Chapter One", "Chapter Two"]));
        assert_eq!(
            work_ids(books),
            [Some(WorkRef::Draft(2)), Some(WorkRef::Draft(3))]
        );

        // Dropping a work does not hand its id to the next one added
        let mut input = store.get(1, 10).unwrap().input.clone();
        input.contents.remove(0);
        input.contents.push(WorkRawInput {
            title: "Gymnopedie No. 2".into(),
            ..WorkRawInput::default()
        });
        let scores = store.save(1, 10, input);
        assert_eq!(work_ids(scores), [Some(WorkRef::Draft(4))]);

        store.record_lookup(1, 10, Lookup::Found);
        assert_eq!(store.get(1, 10).unwrap().lookup, Some(Lookup::Found));

        store.drop_library(1);
        assert!(store.get(1, 10).is_none());
        assert_eq!(work_ids(store.get(2, 20).unwrap()).len(), 2);

        store.forget(2, 20);
        assert!(store.get(2, 20).is_none());
    }
}
