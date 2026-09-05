use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

use crate::identifier;

/// Unsaved review-page edits for one pending import.
///
/// Values are kept as typed; they are parsed only on submit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Draft {
    pub title: String,
    pub publisher: String,
    pub year: String,
    pub identifiers: Vec<IdentifierRow>,
    pub contributors: Vec<ContributorRow>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdentifierRow {
    /// One of the `identifier::Kind` names
    pub kind: String,
    pub value: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContributorRow {
    pub name: String,
    pub role: String,
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
    pub identifiers: Vec<(identifier::Kind, identifier::Normalized)>,
    pub contributors: Vec<ContributorRow>,
}

/// Problems with a draft, one message per field. Empty means it can be submitted.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Errors {
    pub title: Option<String>,
    pub year: Option<String>,
    /// One slot per row, aligned with the draft's rows
    pub identifiers: Vec<Option<String>>,
    pub contributors: Vec<Option<String>>,
}

impl Errors {
    fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.year.is_none()
            && self.identifiers.iter().all(Option::is_none)
            && self.contributors.iter().all(Option::is_none)
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

        let mut identifiers = Vec::new();
        let mut seen = BTreeSet::new();
        errors.identifiers = self
            .identifiers
            .iter()
            .map(|row| {
                let kind: identifier::Kind = match row.kind.parse() {
                    Ok(kind) => kind,
                    Err(_) => return Some("Unknown identifier kind.".to_string()),
                };
                let normalized = match identifier::normalize(kind, &row.value) {
                    Ok(normalized) => normalized,
                    Err(e) => return Some(e.to_string()),
                };
                // The schema is unique on (publication, kind, value), so a second row that
                // normalizes to the same value would fail the accept transaction; reject it here
                // instead.
                if !seen.insert((kind.as_str(), normalized.as_str().to_string())) {
                    return Some("Already listed.".to_string());
                }
                identifiers.push((kind, normalized));
                None
            })
            .collect();

        let mut seen = BTreeSet::new();
        errors.contributors = self
            .contributors
            .iter()
            .map(|row| {
                if row.name.is_empty() {
                    return Some("A name is required.".to_string());
                }
                if row.role.is_empty() {
                    return Some("A role is required.".to_string());
                }
                if !seen.insert((row.name.as_str(), row.role.as_str())) {
                    return Some("Already listed.".to_string());
                }
                None
            })
            .collect();

        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Validated {
            title: self.title.clone(),
            publisher: Some(self.publisher.clone()).filter(|p| !p.is_empty()),
            year,
            identifiers,
            contributors: self.contributors.clone(),
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
            identifiers: vec![
                IdentifierRow {
                    kind: "isbn".into(),
                    value: "978-1-4950-0871-0".into(),
                },
                IdentifierRow {
                    kind: "isbn".into(),
                    value: "0-486-23134-8".into(),
                },
                IdentifierRow {
                    kind: "isbn".into(),
                    value: "978-0-486-23134-1".into(),
                },
            ],
            contributors: vec![
                ContributorRow {
                    name: "Erik Satie".into(),
                    role: String::new(),
                },
                ContributorRow {
                    name: "Erik Satie".into(),
                    role: "composer".into(),
                },
                ContributorRow {
                    name: "Erik Satie".into(),
                    role: "composer".into(),
                },
            ],
        };
        assert_eq!(
            draft.parse().unwrap_err(),
            Errors {
                title: Some("A title is required.".into()),
                year: Some("The year must be a number.".into()),
                identifiers: vec![
                    Some("invalid ISBN".into()),
                    None,
                    Some("Already listed.".into()),
                ],
                contributors: vec![
                    Some("A role is required.".into()),
                    None,
                    Some("Already listed.".into()),
                ],
            }
        );

        let draft = Draft {
            title: "Three gymnopedies".into(),
            publisher: String::new(),
            year: String::new(),
            identifiers: vec![IdentifierRow {
                kind: "isbn".into(),
                value: "0-486-23134-8".into(),
            }],
            contributors: vec![ContributorRow {
                name: "Erik Satie".into(),
                role: "composer".into(),
            }],
        };
        let validated = draft.parse().unwrap();
        assert_eq!(validated.title, "Three gymnopedies");
        assert_eq!(validated.publisher, None);
        assert_eq!(validated.year, None);
        assert_eq!(
            validated.identifiers,
            [(
                identifier::Kind::Isbn,
                identifier::normalize(identifier::Kind::Isbn, "0-486-23134-8").unwrap()
            )]
        );
        assert_eq!(
            validated.contributors,
            [ContributorRow {
                name: "Erik Satie".into(),
                role: "composer".into(),
            }]
        );
    }
}
