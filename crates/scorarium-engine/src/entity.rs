use std::fmt;
use std::future::Future;
use std::hash::Hash;
use std::num::ParseIntError;
use std::str::FromStr;

use scorarium_client::Source;

use crate::audit::{Action, AuditEvent, AuditId, AuditSource, AuditSubject, Audited};
use crate::library::LibraryId;
use crate::person::{Person, PersonId};
use crate::publication::{Publication, PublicationId};
use crate::review::ReviewItem;
use crate::work::{Work, WorkId};
use crate::{Engine, Error, Result};

pub type Timestamp = chrono::DateTime<chrono::Utc>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EntityKind {
    Publication,
    Work,
    Person,
}

impl fmt::Display for EntityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            EntityKind::Publication => "publication",
            EntityKind::Work => "work",
            EntityKind::Person => "person",
        })
    }
}

impl FromStr for EntityKind {
    type Err = Error;

    fn from_str(text: &str) -> Result<EntityKind> {
        Ok(match text {
            "publication" => EntityKind::Publication,
            "work" => EntityKind::Work,
            "person" => EntityKind::Person,
            other => return Err(eyre::eyre!("unknown entity kind {other:?}").into()),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProposalId(i64);

impl ProposalId {
    pub(crate) fn new(id: i64) -> ProposalId {
        ProposalId(id)
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

impl fmt::Display for ProposalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for ProposalId {
    type Err = ParseIntError;

    fn from_str(text: &str) -> std::result::Result<ProposalId, ParseIntError> {
        text.parse().map(ProposalId)
    }
}

/// A proposed edit to an [Entity]
#[derive(Clone, Debug)]
pub struct Proposal<E: Entity> {
    pub id: ProposalId,
    pub source: Source,
    pub created_at: Timestamp,
    pub value: E::Proposal,
}

#[derive(Clone, Debug)]
pub enum AnyEntity {
    Publication(Publication),
    Work(Work),
    Person(Person),
}

impl AnyEntity {
    pub fn id(&self) -> AnyEntityId {
        match self {
            AnyEntity::Publication(publication) => AnyEntityId::Publication(publication.id()),
            AnyEntity::Work(work) => AnyEntityId::Work(work.id()),
            AnyEntity::Person(person) => AnyEntityId::Person(person.id()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnyEntityId {
    Publication(PublicationId),
    Work(WorkId),
    Person(PersonId),
}

impl AnyEntityId {
    pub fn kind(self) -> EntityKind {
        match self {
            AnyEntityId::Publication(_) => EntityKind::Publication,
            AnyEntityId::Work(_) => EntityKind::Work,
            AnyEntityId::Person(_) => EntityKind::Person,
        }
    }

    /// Return the raw database id
    pub fn get(self) -> i64 {
        match self {
            AnyEntityId::Publication(id) => id.get(),
            AnyEntityId::Work(id) => id.get(),
            AnyEntityId::Person(id) => id.get(),
        }
    }
}

/// A trait to define the common bits of what it means to be an entity in the scorarium database.
pub trait Entity: Sized {
    /// A strong type to represent this type of entity in the database
    type Id: Copy + Eq + Hash + fmt::Display + FromStr;
    /// An enum covering the fields that can be updated on an entity
    type Field: Copy + Eq + fmt::Display + FromStr;
    type Proposal: Clone + fmt::Debug;

    /// Load the entity with the given id, if it exists
    fn load(engine: &Engine, id: Self::Id) -> impl Future<Output = Result<Option<Self>>> + Send;

    /// Re-read the entity from the database
    ///
    /// Returns [Error::NotFound] if the entity has been deleted.
    fn reload(&mut self) -> impl Future<Output = Result<()>> + Send;

    fn id(&self) -> Self::Id;
    fn library(&self) -> LibraryId;
    fn is_draft(&self) -> bool;
    fn created_at(&self) -> Timestamp;
    fn last_modified_at(&self) -> Timestamp;

    /// Get an audit log of change events for this entity
    fn audit_log(
        &self,
        before: Option<AuditId>,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<AuditEvent>>> + Send;

    /// Look up this entity in the review queue, if it exists
    fn review_item(&self) -> impl Future<Output = Result<Option<ReviewItem>>> + Send;

    /// List the proposed edits to the entity
    fn proposals(&self) -> impl Future<Output = Result<Vec<Proposal<Self>>>> + Send;

    /// If the entity is a draft, commit it as a non-draft entity
    ///
    /// Returns [Error::NotFound] if the entity has been deleted.
    fn commit(&mut self) -> impl Future<Output = Result<()>> + Send;

    /// Apply the given proposal
    ///
    /// Returns [Error::NotFound] if the entity has been deleted, or the proposal no longer exists
    fn accept(&mut self, proposal: ProposalId) -> impl Future<Output = Result<()>> + Send;

    /// Discard a proposed edit
    ///
    /// Returns [Error::NotFound] if the entity has been deleted, or the proposal no longer exists
    fn reject(&mut self, proposal: ProposalId) -> impl Future<Output = Result<()>> + Send;

    /// Discard every proposed edit, along with any draft they were the last reference to
    ///
    /// Returns [Error::NotFound] if the entity has been deleted.
    fn reject_all(&mut self) -> impl Future<Output = Result<()>> + Send;

    /// Delete the entity and anything orphaned by its deletion
    ///
    /// Returns [Error::NotFound] if the entity has already been deleted.
    fn delete(self) -> impl Future<Output = Result<()>> + Send;
}

/// Commit a rejection, recording an orphan cleanup deletion event for each of `collected`
pub(crate) async fn commit_rejection(tx: Audited<'_>, collected: &[AuditSubject]) -> Result<()> {
    let Some((first, rest)) = collected.split_first() else {
        #[expect(
            deprecated,
            reason = "discarding edit proposals doesn't need to be audited"
        )]
        return tx.commit_unaudited().await;
    };
    let mut tx = tx
        .record(
            AuditSource::OrphanCleanup,
            Action::Deleted,
            Some(first),
            None,
        )
        .await?;
    for subject in rest {
        tx = tx
            .record(
                AuditSource::OrphanCleanup,
                Action::Deleted,
                Some(subject),
                None,
            )
            .await?;
    }
    tx.commit().await
}
