use std::collections::HashMap;
use std::sync::Mutex;

use sqlx::SqlitePool;

use crate::db::pending_import::{self, PendingImport};
use crate::db::publication::{self, NewPublication};
use crate::db::work::{self, NewWork, lead_contributor};
use crate::publication_form::{
    ContributorRow, HoldingRow, IdentifierRow, PublicationForm, PublicationUpdate, WorkRow,
};
use crate::work_form::{WorkForm, WorkUpdate};
use crate::{identifier, publication_form};

impl PublicationForm {
    /// The form a pending import starts from before anything is saved: the holding as entered,
    /// and a valid ISBN or ISMN as the first identifier row, anything else as the title. Derived
    /// on every view rather than stored, so it survives a restart the same way the row does.
    pub fn seed(pending: &PendingImport) -> Self {
        let query = pending.query.trim();
        let mut form = PublicationForm {
            title: String::new(),
            publisher: String::new(),
            year: String::new(),
            holdings: pending
                .holdings
                .iter()
                .map(|h| HoldingRow {
                    id: None,
                    kind: h.kind,
                    location: h.location.clone().unwrap_or_default(),
                })
                .collect(),
            identifiers: Vec::new(),
            contributors: Vec::new(),
            works: Vec::new(),
        };
        for kind in [identifier::Kind::Isbn, identifier::Kind::Ismn] {
            if let Ok(normalized) = identifier::normalize(kind, query) {
                form.identifiers.push(IdentifierRow {
                    kind: kind.as_str().to_string(),
                    value: normalized.as_str().to_string(),
                });
                return form;
            }
        }
        form.title = query.to_string();
        form
    }
}

const HIDDEN_WORK_PROBLEM: &str = "A contributor is incomplete. Open the work to fix it.";

/// Unsaved review-page edits for one pending import.
///
/// The works carry everything the review page's rows do not show, so they are the draft's source
/// of truth: `form.works` is derived from them by [Draft::merge] and never edited directly.
#[derive(Clone)]
pub struct Draft {
    pub form: PublicationForm,
    pub works: Vec<DraftWork>,
    /// Never reused, so a row left over from an earlier view cannot attach to a different work
    next_id: i64,
}

/// A work on a draft, with an id that means nothing outside the draft it belongs to.
#[derive(Clone)]
pub struct DraftWork {
    pub id: i64,
    pub form: WorkForm,
}

impl Draft {
    /// The draft a pending import starts from, before anything is saved: the seeded form and no
    /// works, since nothing has named one yet.
    pub fn seed(pending: &PendingImport) -> Self {
        Draft {
            form: PublicationForm::seed(pending),
            works: Vec::new(),
            next_id: 1,
        }
    }

    /// Fold a submitted form into the draft, so its works keep what the page's rows do not show.
    ///
    /// A row naming a draft work keeps it and sets its title, and edits its lead contributor only
    /// (see [set_lead]). A row with no id, or an id the draft does not have, adds a work whose only
    /// contributor is the row's. Works no row names are dropped. The works end up in row order.
    ///
    /// These are the rules [crate::db::work::write_contents] applies to a publication's stored
    /// works; the two implementations are meant to stay parallel.
    pub fn merge(&mut self, submitted: PublicationForm) {
        let mut works = Vec::new();
        for row in &submitted.works {
            // An id the draft does not have names nothing this form may edit
            let existing = row
                .id
                .and_then(|id| self.works.iter().position(|work| work.id == id))
                .map(|i| self.works.remove(i));
            match existing {
                Some(mut work) => {
                    work.form.title = row.title.clone();
                    set_lead(&mut work.form.contributors, &row.contributor);
                    works.push(work);
                }
                None => {
                    let mut form = WorkForm {
                        title: row.title.clone(),
                        ..WorkForm::default()
                    };
                    set_lead(&mut form.contributors, &row.contributor);
                    works.push(DraftWork {
                        id: self.next_id,
                        form,
                    });
                    self.next_id += 1;
                }
            }
        }
        self.works = works;
        let rows = self.work_rows();
        self.form = PublicationForm {
            works: rows,
            ..submitted
        };
    }

    /// Check the publication form and every work.
    ///
    /// A work whose problem the review page's row does not show gets a message on that row, so a
    /// refused submit is always explainable from the page it was refused on. A row with a problem
    /// of its own keeps its own message, which names a field the user can see.
    #[expect(clippy::result_large_err)]
    pub fn parse(&self) -> Result<(PublicationUpdate, Vec<WorkUpdate>), publication_form::Errors> {
        let mut updates = Vec::new();
        let mut hidden = Vec::new();
        for work in &self.works {
            match work.form.parse() {
                Ok(update) => {
                    updates.push(update);
                    hidden.push(None);
                }
                Err(_) => hidden.push(Some(HIDDEN_WORK_PROBLEM.to_string())),
            }
        }
        let refused = hidden.iter().any(Option::is_some);
        match self.form.parse() {
            Ok(publication) if !refused => Ok((publication, updates)),
            result => {
                let mut errors = result.err().unwrap_or_default();
                if errors.works.len() < hidden.len() {
                    errors.works.resize(hidden.len(), None);
                }
                for (slot, message) in errors.works.iter_mut().zip(hidden) {
                    if slot.is_none() {
                        *slot = message;
                    }
                }
                Err(errors)
            }
        }
    }

    /// The review page's work rows, as the draft's works see them: one row per work, showing the
    /// contributor [crate::db::work::lead_contributor] picks.
    fn work_rows(&self) -> Vec<WorkRow> {
        self.works
            .iter()
            .map(|work| {
                let contributors = &work.form.contributors;
                WorkRow {
                    id: Some(work.id),
                    title: work.form.title.clone(),
                    contributor: lead_contributor(contributors.iter().map(|c| c.role.as_str()))
                        .map(|i| contributors[i].clone())
                        .unwrap_or_default(),
                }
            })
            .collect()
    }
}

/// Apply a work row's one contributor to a draft work's, leaving the ones the row does not show
/// alone: an unchanged lead stays put, a filled row replaces the lead in place and drops any other
/// contributor it now duplicates, and an emptied row removes the lead.
///
/// Unlike [crate::db::work::write_contents], a half-filled row is kept rather than dropped. A
/// draft holds whatever was typed, and validation is what tells the user about it.
fn set_lead(contributors: &mut Vec<ContributorRow>, row: &ContributorRow) {
    let empty = row.name.is_empty() && row.role.is_empty();
    match lead_contributor(contributors.iter().map(|c| c.role.as_str())) {
        Some(i) if contributors[i] == *row => {}
        // The lead is edited rather than replaced, so it keeps its place in the list: that order is
        // what picks the lead for a work with neither a composer nor an author.
        Some(i) if !empty => {
            contributors[i] = row.clone();
            // Another contributor the row now duplicates would credit the same person twice
            let mut index = 0;
            contributors.retain(|c| {
                let keep = index == i || c != row;
                index += 1;
                keep
            });
        }
        Some(i) => {
            contributors.remove(i);
        }
        None if !empty => contributors.push(row.clone()),
        None => {}
    }
}

/// Unsaved review-page edits, by pending import id
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

/// Turn a reviewed import into catalog rows in one transaction and delete the pending import.
///
/// Returns None, having changed nothing, when the pending import no longer exists: a second
/// submit from another tab must not create a second publication.
pub async fn accept(
    pool: &SqlitePool,
    pending: &PendingImport,
    publication: &PublicationUpdate,
    works: &[WorkUpdate],
) -> sqlx::Result<Option<i64>> {
    let library_id = pending.library_id;
    let mut tx = pool.begin().await?;
    let publication_id = publication::create_publication(
        &mut *tx,
        &NewPublication {
            library_id,
            title: &publication.title,
            publisher: publication.publisher.as_deref(),
            year: publication.year,
        },
    )
    .await?;
    publication::write_children(&mut tx, library_id, publication_id, publication).await?;
    // The works are written in full from the draft, not from the thin rows in `publication.works`
    for work in works {
        let work_id = work::create_work(
            &mut *tx,
            &NewWork {
                library_id,
                title: &work.title,
                key: work.key.as_deref(),
                time_signature: work.time_signature.as_deref(),
                instrumentation: work.instrumentation.as_deref(),
            },
        )
        .await?;
        // Added in draft order, which is the order the publication contains them in
        work::add_to_publication(&mut *tx, library_id, publication_id, work_id).await?;
        work::write_contributors(&mut tx, library_id, work_id, &work.contributors).await?;
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
    use crate::db::pending_import::{NewPendingImport, PendingHolding};
    use crate::db::publication::HoldingKind;
    use crate::publication_form::HoldingUpdate;

    fn contributor(name: &str, role: &str) -> ContributorRow {
        ContributorRow {
            name: name.into(),
            role: role.into(),
        }
    }

    fn work_row(id: Option<i64>, title: &str, name: &str, role: &str) -> WorkRow {
        WorkRow {
            id,
            title: title.into(),
            contributor: contributor(name, role),
        }
    }

    /// A submission of the review page's form, carrying nothing but its work rows.
    fn submitted(works: Vec<WorkRow>) -> PublicationForm {
        PublicationForm {
            title: "Album".into(),
            publisher: String::new(),
            year: String::new(),
            holdings: vec![HoldingRow {
                id: None,
                kind: HoldingKind::Physical,
                location: String::new(),
            }],
            identifiers: Vec::new(),
            contributors: Vec::new(),
            works,
        }
    }

    /// A draft work as (id, title, key, [(contributor name, role)])
    type WorkView<'a> = (i64, &'a str, &'a str, Vec<(&'a str, &'a str)>);

    fn works(draft: &Draft) -> Vec<WorkView<'_>> {
        draft
            .works
            .iter()
            .map(|work| {
                let contributors = work
                    .form
                    .contributors
                    .iter()
                    .map(|c| (c.name.as_str(), c.role.as_str()))
                    .collect();
                (
                    work.id,
                    work.form.title.as_str(),
                    work.form.key.as_str(),
                    contributors,
                )
            })
            .collect()
    }

    /// A row edits the work it names without disturbing what the row does not show, and the rows
    /// the review page renders next are derived from the works that result.
    #[test]
    fn merge_keeps_depth() {
        let mut draft = Draft {
            form: submitted(Vec::new()),
            works: vec![DraftWork {
                id: 1,
                form: WorkForm {
                    title: "Prelude".into(),
                    key: "E minor".into(),
                    time_signature: String::new(),
                    instrumentation: String::new(),
                    contributors: vec![
                        contributor("Chopin", "composer"),
                        contributor("Liszt", "arranger"),
                    ],
                },
            }],
            next_id: 2,
        };

        // Renaming the work, leaving its lead alone
        draft.merge(submitted(vec![work_row(
            Some(1),
            "Prelude in E minor",
            "Chopin",
            "composer",
        )]));
        assert_eq!(
            works(&draft),
            [(
                1,
                "Prelude in E minor",
                "E minor",
                vec![("Chopin", "composer"), ("Liszt", "arranger")]
            )],
            "the key and the arranger the row does not show survive"
        );

        // Handing the row to the arranger it already credits
        draft.merge(submitted(vec![work_row(
            Some(1),
            "Prelude in E minor",
            "Liszt",
            "arranger",
        )]));
        assert_eq!(
            works(&draft),
            [(
                1,
                "Prelude in E minor",
                "E minor",
                vec![("Liszt", "arranger")]
            )],
            "the lead is replaced in place, not duplicated"
        );

        // A row naming no work adds one
        draft.merge(submitted(vec![
            work_row(Some(1), "Prelude in E minor", "Liszt", "arranger"),
            work_row(None, "Nocturne", "Field", "composer"),
        ]));
        assert_eq!(
            works(&draft),
            [
                (
                    1,
                    "Prelude in E minor",
                    "E minor",
                    vec![("Liszt", "arranger")]
                ),
                (2, "Nocturne", "", vec![("Field", "composer")]),
            ]
        );
        assert_eq!(
            draft.form.works,
            [
                work_row(Some(1), "Prelude in E minor", "Liszt", "arranger"),
                work_row(Some(2), "Nocturne", "Field", "composer"),
            ],
            "the rows the page renders next name the works they came from"
        );

        // A work no row names is dropped, and its id is not handed to the next work added
        draft.merge(submitted(vec![
            work_row(Some(2), "Nocturne", "Field", "composer"),
            work_row(None, "Mazurka", "", ""),
        ]));
        assert_eq!(
            works(&draft),
            [
                (2, "Nocturne", "", vec![("Field", "composer")]),
                (3, "Mazurka", "", vec![]),
            ]
        );
    }

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
                holdings: &[PendingHolding {
                    kind: HoldingKind::Physical,
                    location: None,
                }],
            },
        )
        .await
        .unwrap();
        let pending = db::pending_import::get(&pool, library_id, pending_id)
            .await
            .unwrap()
            .unwrap();
        let isbn = identifier::normalize(identifier::Kind::Isbn, "0-486-23134-8").unwrap();
        let validated = PublicationUpdate {
            title: "Three gymnopedies".into(),
            publisher: Some("Schirmer".into()),
            year: Some(1888),
            // The form's copies, not the pending row's: the review page may have changed them
            holdings: vec![
                HoldingUpdate {
                    id: None,
                    kind: HoldingKind::Digital,
                    location: Some("satie.pdf".into()),
                },
                HoldingUpdate {
                    id: None,
                    kind: HoldingKind::Physical,
                    location: None,
                },
            ],
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
            works: Vec::new(),
        };

        let works = vec![WorkUpdate {
            title: "Gymnopedie No. 1".into(),
            key: Some("D major".into()),
            time_signature: None,
            instrumentation: None,
            contributors: vec![
                ContributorRow {
                    name: "Erik Satie".into(),
                    role: "composer".into(),
                },
                ContributorRow {
                    name: "Claude Debussy".into(),
                    role: "arranger".into(),
                },
            ],
        }];

        let publication_id = accept(&pool, &pending, &validated, &works)
            .await
            .unwrap()
            .unwrap();

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
        assert_eq!(
            publication
                .holdings
                .iter()
                .map(|h| (h.kind, h.location.as_deref()))
                .collect::<Vec<_>>(),
            [
                (HoldingKind::Digital, Some("satie.pdf")),
                (HoldingKind::Physical, None)
            ]
        );
        let contents = db::work::list_in_publication(&pool, library_id, publication_id)
            .await
            .unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0].title, "Gymnopedie No. 1");
        assert_eq!(contents[0].key.as_deref(), Some("D major"));
        assert_eq!(
            contents[0]
                .contributors
                .iter()
                .map(|c| (c.person_id == satie, c.name.as_str(), c.role.as_str()))
                .collect::<Vec<_>>(),
            [
                (true, "Erik Satie", "composer"),
                (false, "Claude Debussy", "arranger"),
            ],
            "both credits are written, in draft order, reusing the person already in the library"
        );
        assert_eq!(
            db::pending_import::get(&pool, library_id, pending_id)
                .await
                .unwrap(),
            None
        );

        // A second submit of the same import (another tab) must not create a second publication
        assert_eq!(
            accept(&pool, &pending, &validated, &works).await.unwrap(),
            None
        );
        assert_eq!(
            db::publication::list(&pool, library_id)
                .await
                .unwrap()
                .len(),
            1
        );
    }
}
