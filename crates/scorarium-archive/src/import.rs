use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use sqlx::SqliteConnection;

use crate::holding::{Holding, HoldingInput, HoldingRawInput};
use crate::identifier::{self, IdentifierRawInput};
use crate::input::{PersonRef, WorkRef};
use crate::publication::{
    self, Publication, PublicationErrors, PublicationInput, PublicationPost, PublicationRawInput,
};
use crate::summary::{self, PersonSummary};
use crate::work::{self, Work};
use crate::{Action, ArchiveInner, EntityKind, EntityRef, Event, NotFound, Source, draft, person};

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

/// How a source lookup ended, for the review page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lookup {
    Found,
    Failed(String),
}

/// The review page's edits for one pending import, with every work it contains filled in
#[derive(Clone, Debug)]
pub struct DraftPublication {
    pub input: PublicationRawInput,
    /// False when nothing has been saved and the input was seeded just now
    pub saved: bool,
    /// The API lookup outcome, if there was one
    pub lookup: Option<Lookup>,
}

/// What accepting a draft publication resulted in
#[derive(Debug)]
pub enum Accepted {
    Published(Box<Publication>),
    /// The draft was not valid and stays saved with the posted edits, for the review page
    Refused(Box<PublicationErrors>),
}

impl PendingImport {
    /// The saved draft, or one seeded from the entry page when nothing has been saved.
    pub async fn draft(&self) -> crate::Result<DraftPublication> {
        let snapshot = self.archive.drafts().snapshot(self.library_id, self.id);
        let Some(snapshot) = snapshot else {
            return Ok(DraftPublication {
                input: self.initial_draft_contents(),
                saved: false,
                lookup: None,
            });
        };
        let mut input = snapshot.fields;
        let mut tx = self.archive.begin_read().await?;
        for content in snapshot.contents {
            match content {
                draft::Content::Draft(work) => input.contents.push(*work),
                draft::Content::Stored(id) => {
                    let stored = work::load_works(
                        &self.archive,
                        &mut tx,
                        self.library_id,
                        Some(id),
                        None,
                        None,
                    )
                    .await?
                    .pop();
                    input.contents.extend(stored.as_ref().map(Work::raw_input));
                }
            }
        }
        tx.commit().await?;
        Ok(DraftPublication {
            input,
            saved: true,
            lookup: snapshot.lookup,
        })
    }

    pub async fn save_draft(&self, input: PublicationRawInput) -> crate::Result<DraftPublication> {
        let stored: Vec<PersonSummary>;
        // The catalog can have collected a stored work since the page was opened
        let mut vanished = Vec::new();
        {
            let mut conn = self.archive.acquire_read().await?;
            stored = summary::persons(&mut conn, Some(self.library_id), false, None, None)
                .await?
                .into_iter()
                .map(|found| found.summary)
                .collect();
            for work in &input.contents {
                if let Some(WorkRef::Stored(id)) = work.id
                    && !work::work_exists(&mut conn, self.library_id, id).await?
                {
                    vanished.push(id);
                }
            }
        }
        self.archive
            .drafts()
            .save(self.library_id, self.id, input, &vanished, &stored);
        self.draft().await
    }

    /// Merge the review page's post into the draft and store it.
    pub async fn save(&self, post: PublicationPost) -> crate::Result<DraftPublication> {
        let input = self.merge(post).await?;
        self.save_draft(input).await
    }

    async fn merge(&self, post: PublicationPost) -> crate::Result<PublicationRawInput> {
        let mut shown = self.draft().await?.input.contents;
        let mut tx = self.archive.begin_read().await?;
        for id in post.contents.iter().filter_map(|work| work.id) {
            if shown.iter().any(|work| work.id == Some(id)) {
                continue;
            }
            let picked = match id {
                WorkRef::Stored(id) => work::load_works(
                    &self.archive,
                    &mut tx,
                    self.library_id,
                    Some(id),
                    None,
                    None,
                )
                .await?
                .pop()
                .map(|work| work.raw_input()),
                WorkRef::Draft(id) => self.archive.drafts().work(self.library_id, id),
            };
            shown.extend(picked);
        }
        tx.commit().await?;
        Ok(post.merge(shown))
    }

    /// Note what a source lookup produced, so the review page can say so.
    pub fn record_lookup(&self, lookup: Lookup) {
        self.archive
            .drafts()
            .record_lookup(self.library_id, self.id, lookup);
    }

    /// Merge the posted form into the draft and accept it into a publication if it is valid.
    ///
    /// Returns a [NotFound] error when the import was already accepted or discarded.
    pub async fn accept(self, post: PublicationPost) -> crate::Result<Accepted> {
        let saved = self.save(post).await?;
        // A draft that is not ready remains saved with validation errors explaining why it was rejected
        let parsed = match saved.input.parse() {
            Ok(parsed) => parsed,
            Err(errors) => return Ok(Accepted::Refused(errors)),
        };
        let publication = self.accept_into_publication(&parsed).await?;
        Ok(Accepted::Published(Box::new(publication)))
    }

    async fn accept_into_publication(self, input: &PublicationInput) -> crate::Result<Publication> {
        let drafted = self.archive.drafts().persons(self.library_id);
        let mut input = input.clone();
        let mut names = BTreeMap::new();
        for credit in input.contributors_mut() {
            if let PersonRef::Draft(id) = credit.person {
                // A concurrent accept can have relinked and collected it since this draft was saved
                let person = drafted
                    .get(&id)
                    .ok_or_else(|| eyre::eyre!("draft person {id} is gone: {:?}", credit.name))?;
                names.insert(id, person.name.clone());
            }
        }
        let mut audited = self
            .archive
            .begin_audit(Source::User, Event::new(Action::ImportAccepted))
            .await?;
        let mut persons = Vec::with_capacity(names.len());
        for (id, name) in names {
            let stored = person::create_person(&mut audited, self.library_id, &name).await?;
            persons.push((id, stored));
        }
        for credit in input.contributors_mut() {
            if let PersonRef::Draft(id) = credit.person
                && let Some((_, stored)) = persons.iter().find(|(draft, _)| *draft == id)
            {
                credit.person = PersonRef::Linked(*stored);
            }
        }
        let (publication, work_ids) =
            publication::create_publication(&self.archive, &mut audited, self.library_id, &input)
                .await?;
        let result = sqlx::query!(
            "DELETE FROM pending_import WHERE library_id = ? AND id = ?",
            self.library_id,
            self.id
        )
        .execute(&mut *audited)
        .await?;
        if result.rows_affected() == 0 {
            audited.rollback().await?;
            self.forget_draft();
            return Err(NotFound.into());
        }
        audited.set_entity(&publication.entity_ref()).await?;
        audited.commit().await?;
        // Only after the commit, so a rolled-back accept leaves every draft as it was
        let works: Vec<(i64, i64)> = input
            .contents
            .iter()
            .zip(work_ids)
            .filter_map(|(work, stored)| match work.id {
                Some(WorkRef::Draft(draft)) => Some((draft, stored)),
                _ => None,
            })
            .collect();
        self.archive
            .drafts()
            .accept(self.library_id, self.id, &works, &persons);
        Ok(publication)
    }

    /// Delete the import and its draft. Returns a [NotFound] error when it is already gone.
    pub async fn discard(self) -> crate::Result<()> {
        let mut audited = self
            .archive
            .begin_audit(
                Source::User,
                Event::about(Action::ImportDiscarded, self.entity_ref()),
            )
            .await?;
        let result = sqlx::query!(
            "DELETE FROM pending_import WHERE library_id = ? AND id = ?",
            self.library_id,
            self.id
        )
        .execute(&mut *audited)
        .await?;
        if result.rows_affected() == 0 {
            audited.rollback().await?;
            self.forget_draft();
            return Err(NotFound.into());
        }
        audited.commit().await?;
        self.forget_draft();
        Ok(())
    }

    /// Get an EntityRef referring to this entity for use in the audit log
    pub(crate) fn entity_ref(&self) -> EntityRef {
        EntityRef {
            kind: EntityKind::Import,
            id: self.id,
            library_id: Some(self.library_id),
            label: self.query.clone(),
        }
    }

    fn forget_draft(&self) {
        self.archive.drafts().forget(self.library_id, self.id);
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
