use std::collections::HashMap;
use std::sync::Mutex;

use sqlx::SqlitePool;

use crate::db::pending_import::{self, PendingImport};
use crate::db::publication::{self, NewPublication};
use crate::identifier;
use crate::publication_form::{HoldingRow, IdentifierRow, PublicationForm, Validated};

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

/// Unsaved review-page edits, by pending import id
#[derive(Default)]
pub struct DraftStore(Mutex<HashMap<i64, PublicationForm>>);

impl DraftStore {
    pub fn get(&self, pending_id: i64) -> Option<PublicationForm> {
        self.0
            .lock()
            .expect("draft lock poisoned")
            .get(&pending_id)
            .cloned()
    }

    pub fn save(&self, pending_id: i64, draft: PublicationForm) {
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
    publication::write_children(&mut tx, library_id, publication_id, validated).await?;
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
    use crate::publication_form::{ContributorRow, ValidatedHolding};

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
        let validated = Validated {
            title: "Three gymnopedies".into(),
            publisher: Some("Schirmer".into()),
            year: Some(1888),
            // The form's copies, not the pending row's: the review page may have changed them
            holdings: vec![
                ValidatedHolding {
                    id: None,
                    kind: HoldingKind::Digital,
                    location: Some("satie.pdf".into()),
                },
                ValidatedHolding {
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
}
