use std::fmt;
use std::future::Future;
use std::hash::Hash;
use std::str::FromStr;

use crate::audit::{AuditEvent, AuditId};
use crate::library::LibraryId;
use crate::person::Person;
use crate::publication::Publication;
use crate::work::Work;
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

#[derive(Clone, Debug)]
pub enum AnyEntity {
    Publication(Publication),
    Work(Work),
    Person(Person),
}

/// A trait to define the common bits of what it means to be an entity in the scorarium database.
pub trait Entity: Sized {
    /// A strong type to represent this type of entity in the database
    type Id: Copy + Eq + Hash + fmt::Display + FromStr;
    /// An enum covering the fields that can be updated on an entity
    type Field: Copy + Eq + fmt::Display + FromStr;

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

    /// If the entity is a draft, commit it as a non-draft entity
    ///
    /// Returns [Error::NotFound] if the entity has been deleted.
    fn commit(&mut self) -> impl Future<Output = Result<()>> + Send;

    /// Delete the entity and anything orphaned by its deletion
    ///
    /// Returns [Error::NotFound] if the entity has already been deleted.
    fn delete(self) -> impl Future<Output = Result<()>> + Send;
}
