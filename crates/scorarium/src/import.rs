use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

use sqlx::SqlitePool;

use crate::db::pending_import::{self, PendingImport};
use crate::db::person;
use crate::db::publication::{self, HoldingKind, NewPublication};
use crate::identifier;

/// Unsaved review-page edits for one pending import.
///
/// Values are kept as typed; they are parsed only on submit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub title: String,
    pub publisher: String,
    pub year: String,
    pub kind: HoldingKind,
    /// Freeform for physical, a file path for digital; empty means none
    pub location: String,
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

impl Draft {
    /// The draft a pending import starts from before anything is saved: the holding as entered,
    /// and a valid ISBN or ISMN as the first identifier row, anything else as the title. Derived
    /// on every view rather than stored, so it survives a restart the same way the row does.
    pub fn seed(pending: &PendingImport) -> Self {
        let query = pending.query.trim();
        let mut draft = Draft {
            title: String::new(),
            publisher: String::new(),
            year: String::new(),
            kind: pending.kind,
            location: pending.location.clone().unwrap_or_default(),
            identifiers: Vec::new(),
            contributors: Vec::new(),
        };
        for kind in [identifier::Kind::Isbn, identifier::Kind::Ismn] {
            if let Ok(normalized) = identifier::normalize(kind, query) {
                draft.identifiers.push(IdentifierRow {
                    kind: kind.as_str().to_string(),
                    value: normalized.as_str().to_string(),
                });
                return draft;
            }
        }
        draft.title = query.to_string();
        draft
    }
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
    pub kind: HoldingKind,
    pub location: Option<String>,
    pub identifiers: Vec<(identifier::Kind, identifier::Normalized)>,
    pub contributors: Vec<ContributorRow>,
}

/// Problems with a draft, one message per field. Empty means it can be submitted.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Errors {
    pub title: Option<String>,
    pub year: Option<String>,
    pub location: Option<String>,
    /// One slot per row, aligned with the draft's rows
    pub identifiers: Vec<Option<String>>,
    pub contributors: Vec<Option<String>>,
}

impl Errors {
    fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.year.is_none()
            && self.location.is_none()
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
        if self.kind == HoldingKind::Digital && self.location.is_empty() {
            errors.location = Some("Choose a file for a digital copy.".into());
        }

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
            kind: self.kind,
            location: Some(self.location.clone()).filter(|l| !l.is_empty()),
            identifiers,
            contributors: self.contributors.clone(),
        })
    }
}

/// "Erik Satie" sorts as "Satie, Erik". Compound surnames ("Ralph Vaughan Williams") come out
/// wrong and get fixed on the person page; the heuristic only has to be right often enough that
/// the user rarely types a name twice.
pub fn sort_name(name: &str) -> String {
    let mut parts: Vec<&str> = name.split_whitespace().collect();
    match parts.pop() {
        Some(last) if !parts.is_empty() => format!("{last}, {}", parts.join(" ")),
        _ => name.to_string(),
    }
}

/// Turn a reviewed import into catalog rows in one transaction and delete the pending import.
///
/// Returns None, having changed nothing, when the pending import no longer exists: a second
/// submit from another tab must not create a second publication.
pub async fn accept(
    pool: &SqlitePool,
    pending: &PendingImport,
    validated: &Validated,
) -> sqlx::Result<Option<i64>> {
    let library_id = pending.library_id;
    let mut tx = pool.begin().await?;
    let publication_id = publication::create_publication(
        &mut *tx,
        &NewPublication {
            library_id,
            title: &validated.title,
            publisher: validated.publisher.as_deref(),
            year: validated.year,
        },
    )
    .await?;
    for (kind, value) in &validated.identifiers {
        publication::create_identifier(&mut *tx, publication_id, *kind, value).await?;
    }
    publication::create_holding(
        &mut *tx,
        publication_id,
        validated.kind,
        validated.location.as_deref(),
    )
    .await?;
    for row in &validated.contributors {
        // A person created by an earlier row is found by a later one: same transaction
        let person_id = match person::find_by_name(&mut *tx, library_id, &row.name).await? {
            Some(id) => id,
            None => {
                person::create_person(&mut *tx, library_id, &row.name, &sort_name(&row.name))
                    .await?
            }
        };
        person::create_contributor(&mut *tx, library_id, publication_id, person_id, &row.role)
            .await?;
    }
    if !pending_import::delete(&mut *tx, library_id, pending.id).await? {
        tx.rollback().await?;
        return Ok(None);
    }
    tx.commit().await?;
    Ok(Some(publication_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::db::pending_import::NewPendingImport;

    #[sqlx::test]
    async fn accept_creates_publication_once(pool: SqlitePool) {
        let library_id = db::create_library(&pool, "lib").await.unwrap();
        let satie = db::person::create_person(&pool, library_id, "Erik Satie", "Satie, Erik")
            .await
            .unwrap();
        let pending_id = db::pending_import::create(
            &pool,
            &NewPendingImport {
                library_id,
                query: "",
                kind: HoldingKind::Physical,
                location: None,
            },
        )
        .await
        .unwrap();
        let pending = db::pending_import::get(&pool, library_id, pending_id)
            .await
            .unwrap()
            .unwrap();
        let isbn = identifier::normalize(identifier::Kind::Isbn, "0-486-23134-8").unwrap();
        let validated = Validated {
            title: "Three gymnopedies".into(),
            publisher: Some("Schirmer".into()),
            year: Some(1888),
            // The draft's holding, not the pending row's: the review page may have changed it
            kind: HoldingKind::Digital,
            location: Some("satie.pdf".into()),
            identifiers: vec![(identifier::Kind::Isbn, isbn)],
            contributors: vec![
                // An existing person by exact name, and a new one
                ContributorRow {
                    name: "Erik Satie".into(),
                    role: "composer".into(),
                },
                ContributorRow {
                    name: "Claude Debussy".into(),
                    role: "editor".into(),
                },
            ],
        };

        let publication_id = accept(&pool, &pending, &validated).await.unwrap().unwrap();

        let publication = db::publication::get(&pool, library_id, publication_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(publication.title, "Three gymnopedies");
        assert_eq!(publication.publisher.as_deref(), Some("Schirmer"));
        assert_eq!(publication.year, Some(1888));
        assert_eq!(publication.identifiers[0].value, "978-0-486-23134-1");
        assert_eq!(publication.contributors[0].person_id, satie);
        let debussy = db::person::get(&pool, library_id, publication.contributors[1].person_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(debussy.sort_name, "Debussy, Claude");
        assert_eq!(publication.holdings[0].kind, HoldingKind::Digital);
        assert_eq!(
            publication.holdings[0].location.as_deref(),
            Some("satie.pdf")
        );
        assert_eq!(
            db::pending_import::get(&pool, library_id, pending_id)
                .await
                .unwrap(),
            None
        );

        // A second submit of the same import (another tab) must not create a second publication
        assert_eq!(accept(&pool, &pending, &validated).await.unwrap(), None);
        assert_eq!(
            db::publication::list(&pool, library_id)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn draft_errors() {
        let draft = Draft {
            title: String::new(),
            publisher: String::new(),
            year: "abc".into(),
            kind: HoldingKind::Digital,
            location: String::new(),
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
                location: Some("Choose a file for a digital copy.".into()),
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
            kind: HoldingKind::Physical,
            location: String::new(),
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
        assert_eq!(validated.kind, HoldingKind::Physical);
        assert_eq!(validated.location, None);
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
