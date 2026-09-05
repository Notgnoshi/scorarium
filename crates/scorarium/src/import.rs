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

/// A draft's typed values, ready to become catalog rows.
#[derive(Debug, PartialEq, Eq)]
pub struct Validated {
    pub title: String,
    pub publisher: Option<String>,
    pub year: Option<i64>,
}

/// Problems with a draft, one message per field. Empty means it can be submitted.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Errors {
    pub title: Option<String>,
    pub year: Option<String>,
}

impl Errors {
    fn is_empty(&self) -> bool {
        self.title.is_none() && self.year.is_none()
    }
}

impl Draft {
    /// Check and convert the draft. Every problem is reported, not just the first.
    pub fn parse(&self) -> Result<Validated, Errors> {
        let mut errors = Errors::default();
        if self.title.is_empty() {
            errors.title = Some("A title is required.".into());
        }
        let year = if self.year.is_empty() {
            None
        } else {
            match self.year.parse::<i64>() {
                Ok(year) => Some(year),
                Err(_) => {
                    errors.year = Some("The year must be a number.".into());
                    None
                }
            }
        };
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Validated {
            title: self.title.clone(),
            publisher: Some(self.publisher.clone()).filter(|p| !p.is_empty()),
            year,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_errors() {
        let draft = Draft {
            title: String::new(),
            publisher: String::new(),
            year: "abc".into(),
        };
        assert_eq!(
            draft.parse().unwrap_err(),
            Errors {
                title: Some("A title is required.".into()),
                year: Some("The year must be a number.".into()),
            }
        );

        let draft = Draft {
            title: "Three gymnopedies".into(),
            publisher: String::new(),
            year: String::new(),
        };
        let validated = draft.parse().unwrap();
        assert_eq!(validated.title, "Three gymnopedies");
        assert_eq!(validated.publisher, None);
        assert_eq!(validated.year, None);
    }
}
