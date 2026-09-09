//! The scorarium data model
//!
//! [Archive] is the top-level entity. It contains [Library]s, against which most other data access
//! is performed.
mod audit;
mod catalog;
mod demo;
mod holding;
pub mod identifier;
mod import;
mod input;
mod library;
mod password;
mod person;
mod publication;
mod work;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sqlx::pool::PoolConnection;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Sqlite, SqlitePool, Transaction};

pub use crate::audit::{Action, AuditEntry, EntityKind, EntityRef, Event, Field, Source};
pub use crate::catalog::{CatalogNumber, Similarity};
pub use crate::holding::{
    Holding, HoldingErrors, HoldingInput, HoldingKind, HoldingRawInput, parse_holdings,
};
pub use crate::identifier::{Identifier, IdentifierRawInput};
pub use crate::import::{Draft, PendingImport};
pub use crate::input::{ContributorInput, ValidationError};
pub use crate::library::Library;
pub use crate::password::PasswordCheck;
pub use crate::person::{Contributor, Person};
pub use crate::publication::{
    Publication, PublicationErrors, PublicationInput, PublicationRawInput,
};
pub use crate::work::{CatalogNumberEntry, Work, WorkErrors, WorkInput, WorkRawInput};

pub type Result<T> = eyre::Result<T>;

/// An error to indicate something as not found
///
/// Most often, methods in the [Archive] data model will return an eyre [Result], so if consumers
/// care about distinguishing [NotFound] from other errors, they should use
/// `e.downcast_ref::<NotFound>()`
#[derive(Debug)]
pub struct NotFound;

impl std::fmt::Display for NotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not found")
    }
}

impl std::error::Error for NotFound {}

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

#[derive(Debug)]
pub struct Archive {
    shared: Arc<ArchiveInner>,
}

/// What the archive and every handle it hands out share.
#[derive(Debug)]
pub(crate) struct ArchiveInner {
    pool: SqlitePool,
    pub drafts: Mutex<HashMap<i64, import::SavedDraft>>,
}

// database access
impl ArchiveInner {
    /// Open a write transaction, recording what it is for before anything else happens.
    ///
    /// This is intended to be the only mechanism the [Archive]'s [SqlitePool] is available,
    /// strongly encouraging all database interactions in this crate to land in the audit log.
    pub(crate) async fn begin_audit(
        &self,
        source: Source,
        event: Event,
    ) -> Result<audit::Audited<'_>> {
        let mut tx = self.pool.begin().await?;
        let group = audit::insert(&mut tx, None, source, &event).await?;
        audit::trim(&mut tx).await?;
        Ok(audit::Audited::new(tx, group))
    }

    /// An un-audited transaction for reads that need one snapshot across several queries
    pub(crate) async fn begin_read(&self) -> Result<Transaction<'_, Sqlite>> {
        Ok(self.pool.begin().await?)
    }

    /// An un-audited connection for a single read
    pub(crate) async fn acquire_read(&self) -> Result<PoolConnection<Sqlite>> {
        Ok(self.pool.acquire().await?)
    }
}

// construction
impl Archive {
    /// Open the archive in the given data directory, creating and migrating it as necessary.
    pub async fn open(data_dir: &Path) -> Result<Archive> {
        std::fs::create_dir_all(data_dir)?;
        let options = SqliteConnectOptions::new()
            .filename(data_dir.join("scorarium.db"))
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePool::connect_with(options).await?;
        MIGRATOR.run(&pool).await?;
        Ok(Archive::new(pool))
    }

    /// A fresh, migrated, empty archive that lives in-memory
    pub async fn in_memory() -> Result<Archive> {
        let options = SqliteConnectOptions::new()
            .in_memory(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1) // sqlite creates a new in-memory database per-connection
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await?;
        MIGRATOR.run(&pool).await?;
        Ok(Archive::new(pool))
    }

    fn new(pool: SqlitePool) -> Archive {
        Archive {
            shared: Arc::new(ArchiveInner {
                pool,
                drafts: Mutex::default(),
            }),
        }
    }

    /// Fill an empty archive with the demo libraries that `--demo` serves
    pub async fn populate_demo(&self) -> Result<()> {
        demo::populate(self).await
    }

    pub(crate) fn shared(&self) -> &Arc<ArchiveInner> {
        &self.shared
    }
}

impl Archive {
    /// Get the database's audit log, most recent entries first.
    pub async fn audit_log(&self) -> Result<Vec<AuditEntry>> {
        let mut conn = self.shared.acquire_read().await?;
        audit::load_entries(&mut conn).await
    }
}

// libraries
impl Archive {
    /// Get all libraries that exist
    pub async fn libraries(&self) -> Result<Vec<Library>> {
        let mut conn = self.shared.acquire_read().await?;
        library::list_libraries(&self.shared, &mut conn).await
    }

    /// Get the given library, if it exists
    pub async fn library(&self, id: i64) -> Result<Option<Library>> {
        let mut conn = self.shared.acquire_read().await?;
        library::get_library(&self.shared, &mut conn, id).await
    }

    /// Create a library with the given name
    ///
    /// Names need not be unique.
    pub async fn create_library(&self, name: &str) -> Result<Library> {
        let mut audited = self
            .shared
            .begin_audit(Source::User, Event::new(Action::Created))
            .await?;
        let library = library::create_library(&self.shared, &mut audited, name).await?;
        audited.set_entity(&library.entity_ref()).await?;
        audited.commit().await?;
        Ok(library)
    }

    /// Every catalog number in every library, private works included, for the settings page
    pub async fn all_catalog_numbers(&self) -> Result<Vec<CatalogNumberEntry>> {
        let mut conn = self.shared.acquire_read().await?;
        work::load_catalog_numbers(&mut conn, None, None, false).await
    }

    /// Every library's pending imports, oldest first
    pub async fn pending_imports(&self) -> Result<Vec<PendingImport>> {
        let mut tx = self.shared.begin_read().await?;
        let queue = import::load_pending_imports(&self.shared, &mut tx, None, None).await?;
        tx.commit().await?;
        Ok(queue)
    }

    /// How many imports await review, for the header
    pub async fn pending_import_count(&self) -> Result<i64> {
        let mut conn = self.shared.acquire_read().await?;
        let count = sqlx::query_scalar!("SELECT COUNT(*) FROM pending_import")
            .fetch_one(&mut *conn)
            .await?;
        Ok(count)
    }
}

// auth
impl Archive {
    /// Whether the admin password has been claimed by the first login attempt
    pub async fn password_claimed(&self) -> Result<bool> {
        let mut conn = self.shared.acquire_read().await?;
        Ok(password::stored_password_hash(&mut conn).await?.is_some())
    }

    /// Claim the admin password
    ///
    /// Returns false (and fails) when a password was already claimed, so that two racing first
    /// logins cannot both win.
    pub async fn claim_password(&self, password: &str) -> Result<bool> {
        let hash = password::hash_password(password)?;
        let mut audited = self
            .shared
            .begin_audit(Source::User, Event::new(Action::PasswordClaimed))
            .await?;
        let claimed = password::insert_password_hash(&mut audited, &hash).await?;
        if claimed {
            audited.commit().await?;
        } else {
            audited.rollback().await?;
        }
        Ok(claimed)
    }

    /// Check the given password against the salted and hashed stored password
    pub async fn verify_password(&self, password: &str) -> Result<PasswordCheck> {
        let mut conn = self.shared.acquire_read().await?;
        let Some(stored) = password::stored_password_hash(&mut conn).await? else {
            return Ok(PasswordCheck::Unclaimed);
        };
        password::verify_password(&stored, password)
    }

    /// Change the admin password to the given password
    ///
    /// A no-op while the password is unclaimed, so that a racing client cannot claim it this way.
    /// The caller is expected to ensure that the user has verified their old password before
    /// allowing them to change it.
    pub async fn change_password(&self, password: &str) -> Result<()> {
        let hash = password::hash_password(password)?;
        let mut audited = self
            .shared
            .begin_audit(Source::User, Event::new(Action::PasswordChanged))
            .await?;
        password::update_password_hash(&mut audited, &hash).await?;
        audited.commit().await
    }
}
