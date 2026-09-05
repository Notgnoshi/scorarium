use std::collections::HashMap;
use std::sync::Mutex;

/// Unsaved review-page edits for one pending import.
///
/// Values are kept as typed; they are parsed only on submit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Draft {
    pub title: String,
    pub publisher: String,
    pub year: String,
}

/// Drafts by pending import id
#[derive(Default)]
pub struct DraftStore(Mutex<HashMap<i64, Draft>>);

impl DraftStore {
    pub fn get(&self, pending_id: i64) -> Option<Draft> {
        self.0
            .lock()
            .expect("draft lock poisoned")
            .get(&pending_id)
            .cloned()
    }

    pub fn save(&self, pending_id: i64, draft: Draft) {
        self.0
            .lock()
            .expect("draft lock poisoned")
            .insert(pending_id, draft);
    }

    pub fn remove(&self, pending_id: i64) {
        self.0
            .lock()
            .expect("draft lock poisoned")
            .remove(&pending_id);
    }
}
