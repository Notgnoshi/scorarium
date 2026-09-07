use std::collections::HashMap;
use std::sync::Arc;

use sqlx::SqliteConnection;

use crate::holding::{Holding, HoldingInput, HoldingRawInput};
use crate::identifier::{self, IdentifierRawInput};
use crate::publication::{self, Publication, PublicationInput, PublicationRawInput};
use crate::{ArchiveInner, NotFound};

/// An import the user has started but has not yet accepted or discarded
#[derive(Clone, Debug)]
pub struct PendingImport {
    pub id: i64,
    pub library_id: i64,
    pub library_name: String,
    /// The identifier or title the user entered to start the import
    pub query: String,
    /// The copies entered on the entry page, in the order entered
    pub holdings: Vec<Holding>,
    /// Unix timestamp
    pub created_at: i64,
    archive: Arc<ArchiveInner>,
}

/// The review page's edits for one pending import
#[derive(Clone, Debug)]
pub struct Draft {
    pub input: PublicationRawInput,
    /// False when nothing has been saved and the input was seeded just now
    pub saved: bool,
}

/// A draft as the archive keeps it, alongside what it needs to name the next work.
#[derive(Debug)]
pub(crate) struct SavedDraft {
    input: PublicationRawInput,
    // Never reused, so a work id left over from an earlier view of the page cannot attach itself
    // to a work added since
    next_work_id: i64,
}

impl PendingImport {
    /// The saved draft, or one seeded from the entry page when nothing has been saved.
    pub fn draft(&self) -> Draft {
        match self.saved_drafts().get(&self.id) {
            Some(saved) => Draft {
                input: saved.input.clone(),
                saved: true,
            },
            None => Draft {
                input: self.initial_draft_contents(),
                saved: false,
            },
        }
    }

    /// Store the review page's edits, and return what was stored.
    pub fn save_draft(&self, mut input: PublicationRawInput) -> Draft {
        let mut drafts = self.saved_drafts();
        let saved = drafts.entry(self.id).or_insert_with(|| SavedDraft {
            input: PublicationRawInput::default(),
            next_work_id: 1,
        });
        for work in &mut input.contents {
            if work.id.is_none() {
                work.id = Some(saved.next_work_id);
                saved.next_work_id += 1;
            }
        }
        saved.input = input.clone();
        Draft { input, saved: true }
    }

    /// Create the publication this import became, and delete the import, in one transaction.
    ///
    /// Returns a [NotFound] error when the import was already accepted or discarded.
    pub async fn accept_into_publication(
        self,
        input: &PublicationInput,
    ) -> crate::Result<Publication> {
        let mut tx = self.archive.pool.begin().await?;
        let publication =
            publication::create_publication(&self.archive, &mut tx, self.library_id, input).await?;
        let result = sqlx::query!(
            "DELETE FROM pending_import WHERE library_id = ? AND id = ?",
            self.library_id,
            self.id
        )
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Err(NotFound.into());
        }
        tx.commit().await?;
        self.saved_drafts().remove(&self.id);
        Ok(publication)
    }

    /// Delete the import and its draft. Returns a [NotFound] error when it is already gone.
    pub async fn discard(self) -> crate::Result<()> {
        let result = sqlx::query!(
            "DELETE FROM pending_import WHERE library_id = ? AND id = ?",
            self.library_id,
            self.id
        )
        .execute(&self.archive.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(NotFound.into());
        }
        self.saved_drafts().remove(&self.id);
        Ok(())
    }

    /// What the review page opens with before anything is saved: the copies as they were entered,
    /// and the query as an identifier when it is a valid ISBN or ISMN, else as the title.
    ///
    /// Derived on every view rather than stored, so it survives a restart the way the import does.
    fn initial_draft_contents(&self) -> PublicationRawInput {
        let query = self.query.trim();
        let mut input = PublicationRawInput {
            holdings: self
                .holdings
                .iter()
                .map(|holding| HoldingRawInput {
                    id: None,
                    kind: holding.kind,
                    location: holding.location.clone().unwrap_or_default(),
                })
                .collect(),
            ..PublicationRawInput::default()
        };
        for kind in [identifier::Kind::Isbn, identifier::Kind::Ismn] {
            if let Ok(normalized) = identifier::normalize(kind, query) {
                input.identifiers.push(IdentifierRawInput {
                    kind: kind.as_str().to_string(),
                    value: normalized.as_str().to_string(),
                });
                return input;
            }
        }
        input.title = query.to_string();
        input
    }

    fn saved_drafts(&self) -> std::sync::MutexGuard<'_, HashMap<i64, SavedDraft>> {
        self.archive.drafts.lock().expect("draft lock poisoned")
    }
}

/// Start an import, with the copies entered on the entry page.
pub(crate) async fn start_import(
    shared: &Arc<ArchiveInner>,
    conn: &mut SqliteConnection,
    library_id: i64,
    query: &str,
    holdings: &[HoldingInput],
) -> crate::Result<PendingImport> {
    let query = query.trim();
    let created = sqlx::query!(
        "INSERT INTO pending_import (library_id, query) VALUES (?, ?)",
        library_id,
        query,
    )
    .execute(&mut *conn)
    .await?;
    let id = created.last_insert_rowid();
    for holding in holdings {
        let kind = holding.kind.as_str();
        sqlx::query!(
            "INSERT INTO pending_import_holding (pending_import_id, kind, location)
             VALUES (?, ?, ?)",
            id,
            kind,
            holding.location,
        )
        .execute(&mut *conn)
        .await?;
    }
    let import = load_pending_imports(shared, conn, None, Some(id))
        .await?
        .pop()
        .expect("the import was just created on this transaction");
    Ok(import)
}

/// Pending imports in one library, or in every library, oldest first.
pub(crate) async fn load_pending_imports(
    shared: &Arc<ArchiveInner>,
    conn: &mut SqliteConnection,
    library_id: Option<i64>,
    id: Option<i64>,
) -> crate::Result<Vec<PendingImport>> {
    let mut imports: Vec<PendingImport> = sqlx::query!(
        "SELECT p.id, p.library_id, l.name AS library_name, p.query, p.created_at
         FROM pending_import p JOIN library l ON l.id = p.library_id
         WHERE (?1 IS NULL OR p.library_id = ?1) AND (?2 IS NULL OR p.id = ?2)
         ORDER BY p.created_at, p.id",
        library_id,
        id
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|row| PendingImport {
        id: row.id,
        library_id: row.library_id,
        library_name: row.library_name,
        query: row.query,
        holdings: Vec::new(),
        created_at: row.created_at,
        archive: shared.clone(),
    })
    .collect();
    let index: HashMap<i64, usize> = imports
        .iter()
        .enumerate()
        .map(|(i, import)| (import.id, i))
        .collect();

    let holdings = sqlx::query!(
        "SELECT id, pending_import_id, kind, location FROM pending_import_holding
         WHERE pending_import_id IN
            (SELECT id FROM pending_import
             WHERE (?1 IS NULL OR library_id = ?1) AND (?2 IS NULL OR id = ?2))
         ORDER BY id",
        library_id,
        id
    )
    .fetch_all(&mut *conn)
    .await?;
    for row in holdings {
        imports[index[&row.pending_import_id]]
            .holdings
            .push(Holding {
                id: row.id,
                kind: row.kind.parse().map_err(eyre::Report::msg)?,
                location: row.location,
            });
    }
    Ok(imports)
}

/// The ids of a library's pending imports, so that deleting the library can drop their drafts.
pub(crate) async fn pending_import_ids(
    conn: &mut SqliteConnection,
    library_id: i64,
) -> crate::Result<Vec<i64>> {
    let ids = sqlx::query_scalar!(
        "SELECT id FROM pending_import WHERE library_id = ?",
        library_id
    )
    .fetch_all(conn)
    .await?;
    Ok(ids)
}
