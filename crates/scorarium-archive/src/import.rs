use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::external_id::EntityKind;
use crate::holding::{Holding, HoldingRawInput};
use crate::identifier::{self, IdentifierRawInput};
use crate::input::{ContributorInput, PersonRef, WorkRef};
use crate::library::Library;
use crate::person::{ExternalPerson, PersonRawInput};
use crate::publication::{
    self, Publication, PublicationErrors, PublicationInput, PublicationPost, PublicationRawInput,
};
use crate::summary::{self, PersonSummary};
use crate::work::{self, Work, WorkMerges};
use crate::{Action, ArchiveInner, Field, NotFound, Source, draft, person};

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
    pub(crate) archive: Arc<ArchiveInner>,
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
        let lookup = snapshot.lookup.clone();
        Ok(DraftPublication {
            input: self.fill(snapshot).await?,
            saved: true,
            lookup,
        })
    }

    /// The snapshot's input with its stored works loaded in
    async fn fill(&self, snapshot: draft::Snapshot) -> crate::Result<PublicationRawInput> {
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
        Ok(input)
    }

    pub async fn save_draft(&self, input: PublicationRawInput) -> crate::Result<DraftPublication> {
        self.store(input, Vec::new()).await
    }

    /// Add contributors a lookup found to the draft
    pub async fn add_external_contributors(
        &self,
        found: Vec<(ExternalPerson, String)>,
    ) -> crate::Result<DraftPublication> {
        let input = self.draft().await?.input;
        self.store(input, found).await
    }

    async fn store(
        &self,
        mut input: PublicationRawInput,
        found: Vec<(ExternalPerson, String)>,
    ) -> crate::Result<DraftPublication> {
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
        {
            let mut drafts = self.archive.drafts();
            for (person, role) in found {
                let name = person.name.clone();
                let Some(id) = drafts.add_external_person(self.library_id, self.id, person) else {
                    return Err(NotFound.into());
                };
                input.contributors.push(ContributorInput {
                    name,
                    role,
                    person: PersonRef::Unresolved,
                    external_person: Some(id),
                });
            }
            if !drafts.save(self.library_id, self.id, input, &vanished, &stored) {
                return Err(NotFound.into());
            }
        }
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
    /// Returns a [NotFound] error when the import was already accepted or discarded, or when
    /// another accept of it is in flight.
    pub async fn accept(self, post: PublicationPost) -> crate::Result<Accepted> {
        self.save(post).await?;
        let (snapshot, drafted, external) = {
            let mut drafts = self.archive.drafts();
            let Some(snapshot) = drafts.begin_accept(self.library_id, self.id) else {
                return Err(NotFound.into());
            };
            (
                snapshot,
                drafts.persons(self.library_id),
                drafts.external_persons(self.library_id, self.id),
            )
        };
        let mut claim = Accepting {
            archive: self.archive.clone(),
            library_id: self.library_id,
            id: self.id,
            armed: true,
        };
        let input = self.fill(snapshot).await?;
        // A draft that is not ready remains saved with validation errors explaining why it was rejected
        let parsed = match input.parse() {
            Ok(parsed) => parsed,
            Err(errors) => return Ok(Accepted::Refused(errors)),
        };
        let publication = self
            .accept_into_publication(&parsed, drafted, external, &mut claim)
            .await?;
        drop(claim);
        Ok(Accepted::Published(Box::new(publication)))
    }

    async fn accept_into_publication(
        self,
        input: &PublicationInput,
        drafted: HashMap<i64, PersonRawInput>,
        external: HashMap<i64, ExternalPerson>,
        claim: &mut Accepting,
    ) -> crate::Result<Publication> {
        let mut input = input.clone();
        let mut new_persons: BTreeMap<i64, (String, Vec<String>)> = BTreeMap::new();
        // The links lookups found for each stored person this import refers to
        let mut additions: BTreeMap<i64, Vec<String>> = BTreeMap::new();
        for contributor in input.contributors_mut() {
            // A reference that matches nothing in this import carries nothing
            let found = contributor
                .external_person
                .and_then(|id| external.get(&id))
                .map(|found| found.links.as_slice())
                .unwrap_or_default();
            match contributor.person {
                PersonRef::Draft(id) => {
                    // A concurrent accept can have relinked and collected it since this draft
                    // was saved
                    let person = drafted.get(&id).ok_or_else(|| {
                        eyre::eyre!("draft person {id} is gone: {:?}", contributor.name)
                    })?;
                    let (_, links) = new_persons
                        .entry(id)
                        .or_insert_with(|| (person.name.clone(), Vec::new()));
                    links.extend(found.iter().cloned());
                }
                PersonRef::Linked(id) if !found.is_empty() => {
                    additions
                        .entry(id)
                        .or_default()
                        .extend(found.iter().cloned());
                }
                _ => {}
            }
        }
        let mut audited = self.archive.begin_write().await?;
        let mut persons = Vec::with_capacity(new_persons.len());
        for (id, (name, links)) in new_persons {
            let links = crate::input::valid_links(EntityKind::Person, &links);
            let stored =
                person::create_person(&mut audited, self.library_id, &name, &links).await?;
            persons.push((id, stored));
        }
        for credit in input.contributors_mut() {
            if let PersonRef::Draft(id) = credit.person
                && let Some((_, stored)) = persons.iter().find(|(draft, _)| *draft == id)
            {
                credit.person = PersonRef::Linked(*stored);
            }
        }
        let mut merges = WorkMerges::default();
        let (publication, work_ids) = publication::create_publication(
            &self.archive,
            &mut audited,
            self.library_id,
            &input,
            &mut merges,
        )
        .await?;
        for (id, links) in additions {
            let links = crate::input::valid_links(EntityKind::Person, &links);
            if person::add_missing_links(&mut audited, id, &links).await? == 0 {
                continue;
            }
            let gained =
                person::load_persons(&self.archive, &mut audited, self.library_id, Some(id), None)
                    .await?
                    .pop()
                    .ok_or_else(|| eyre::eyre!("person {id} is gone after gaining links"))?;
            audited
                .record(
                    Source::User,
                    Action::Updated,
                    Some(gained.entity_ref()),
                    &[Field::Links],
                )
                .await?;
        }
        audited
            .record(
                Source::User,
                Action::ImportAccepted,
                Some(publication.entity_ref()),
                &[],
            )
            .await?;
        claim.armed = false;
        audited.commit().await.inspect_err(|_| claim.armed = true)?;
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
        {
            let mut drafts = self.archive.drafts();
            drafts.accept(self.library_id, self.id, &works, &persons);
            drafts.relink_works(merges.as_slice());
        }
        Ok(publication)
    }

    /// Delete the import and its draft. Returns a [NotFound] error when it is already gone or an
    /// accept of it is in flight.
    pub fn discard(self) -> crate::Result<()> {
        if self.archive.drafts().forget(self.library_id, self.id) {
            Ok(())
        } else {
            Err(NotFound.into())
        }
    }

    /// What the review page opens with before anything is saved: the copies as they were entered,
    /// and the query as an identifier when it is a valid ISBN or ISMN, else as the title.
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

/// One accept's claim on an import.
struct Accepting {
    archive: Arc<ArchiveInner>,
    library_id: i64,
    id: i64,
    armed: bool,
}

impl Drop for Accepting {
    fn drop(&mut self) {
        if self.armed {
            self.archive.drafts().end_accept(self.library_id, self.id);
        }
    }
}

/// Pending imports in one library, or in every one of `libraries`, oldest first.
///
/// An entry whose library is not among those given is skipped: its library was deleted after the
/// import started, and nothing can reach it any more.
pub(crate) fn pending_imports(
    shared: &Arc<ArchiveInner>,
    libraries: &[Library],
    library_id: Option<i64>,
    id: Option<i64>,
) -> Vec<PendingImport> {
    let mut imports: Vec<PendingImport> = shared
        .drafts()
        .imports(library_id)
        .filter(|(_, entry_id, _)| id.is_none_or(|wanted| wanted == *entry_id))
        .filter_map(|(library_id, id, entry)| {
            let library = libraries.iter().find(|library| library.id == library_id)?;
            Some(PendingImport {
                id,
                library_id,
                library_name: library.name.clone(),
                query: entry.query.clone(),
                holdings: entry.holdings.clone(),
                created_at: entry.created_at,
                archive: shared.clone(),
            })
        })
        .collect();
    // Ids are handed out in creation order, and the store iterates libraries in hash order
    imports.sort_by_key(|import| import.id);
    imports
}
