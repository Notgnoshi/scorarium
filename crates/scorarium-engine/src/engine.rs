use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use scorarium_client::{Client, UserAgent};
use sqlx::pool::PoolConnection;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::{ConnectOptions, Sqlite, SqlitePool};

use crate::audit::Audited;
use crate::enrich::Task;
use crate::entity::AnyEntityId;
use crate::{Error, Result};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

#[derive(Debug)]
pub struct EngineOptions {
    migrate_on_startup: bool,
    client: Client,
}

impl EngineOptions {
    /// Build default [EngineOptions] that uses the given [UserAgent]
    ///
    /// Providing contact information (an email) in the user agent is recommended because some APIs
    /// increase their rate limits if contact info is provided.
    pub fn new(user_agent: UserAgent) -> Result<EngineOptions> {
        let client = Client::new(user_agent).map_err(Error::Generic)?;
        Ok(EngineOptions {
            migrate_on_startup: false,
            client,
        })
    }

    /// Run migrations on startup, otherwise startup on an outdated database will fail.
    ///
    /// A missing database is always created and migrated.
    pub fn migrate_on_startup(mut self, migrate: bool) -> EngineOptions {
        self.migrate_on_startup = migrate;
        self
    }

    /// Dependency inject the API client
    pub fn client(mut self, client: Client) -> EngineOptions {
        self.client = client;
        self
    }
}

#[derive(Clone, Debug)]
pub struct Engine {
    inner: Arc<EngineInner>,
}

#[derive(Debug)]
struct EngineInner {
    pool: SqlitePool,
    client: Client,
    enrichments: Mutex<HashMap<AnyEntityId, Arc<Task>>>,
    _temp_dir: Option<tempfile::TempDir>,
}

impl Engine {
    /// Open the engine in `data_dir`, creating the directory and database as necessary
    pub async fn open(data_dir: impl AsRef<Path>, options: EngineOptions) -> Result<Engine> {
        let data_dir = data_dir.as_ref();
        std::fs::create_dir_all(data_dir)?;
        Engine::new(data_dir, options.migrate_on_startup, options.client, None).await
    }

    /// Open a temporary engine filled with the demo libraries
    ///
    /// The temporary directory is cleaned up on shutdown. A fresh database is always migrated, so
    /// [EngineOptions::migrate_on_startup] has no effect.
    pub async fn demo(options: EngineOptions) -> Result<Engine> {
        let dir = tempfile::tempdir()?;
        let data_dir = dir.path().to_path_buf();
        let engine = Engine::new(&data_dir, true, options.client, Some(dir)).await?;
        crate::demo::populate(&engine).await?;
        Ok(engine)
    }

    async fn new(
        data_dir: &Path,
        migrate_on_startup: bool,
        client: Client,
        temp_dir: Option<tempfile::TempDir>,
    ) -> Result<Engine> {
        let path = data_dir.join("scorarium.db");
        let migrate = migrate_on_startup || !path.exists();
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(5))
            .log_statements(log::LevelFilter::Trace)
            .log_slow_statements(log::LevelFilter::Warn, Duration::from_millis(100));
        let pool = SqlitePool::connect_with(options).await?;
        check_schema_version(&pool, migrate).await?;
        if migrate {
            MIGRATOR.run(&pool).await.map_err(eyre::Report::new)?;
        }
        Ok(Engine {
            inner: Arc::new(EngineInner {
                pool,
                client,
                enrichments: Mutex::default(),
                _temp_dir: temp_dir,
            }),
        })
    }

    pub(crate) fn client(&self) -> &Client {
        &self.inner.client
    }

    /// Get the enrichment tasks in progress
    pub(crate) fn enrichments(&self) -> MutexGuard<'_, HashMap<AnyEntityId, Arc<Task>>> {
        self.inner
            .enrichments
            .lock()
            .expect("enrichment registry lock poisoned")
    }

    /// Acquire a read-only connection for queries
    pub(crate) async fn begin_read(&self) -> Result<PoolConnection<Sqlite>> {
        // read-only is by convention. It's possible to enforce, but that'd require per-transaction
        // pragmas, or multiple pools.
        Ok(self.inner.pool.acquire().await?)
    }

    /// Begin a write transaction that must record an audit event before it can be committed
    pub(crate) async fn begin_write(&self) -> Result<Audited<'static>> {
        Ok(Audited::new(
            self.inner.pool.begin_with("BEGIN IMMEDIATE").await?,
        ))
    }
}

async fn check_schema_version(pool: &SqlitePool, migrate: bool) -> Result<()> {
    use sqlx::migrate::Migrate;

    let mut conn = pool.acquire().await?;
    // A new database has no migrations table until the migrator creates one.
    if migrate {
        conn.ensure_migrations_table(&MIGRATOR.table_name)
            .await
            .map_err(eyre::Report::new)?;
    }
    if let Some(version) = conn
        .dirty_version(&MIGRATOR.table_name)
        .await
        .map_err(eyre::Report::new)?
    {
        return Err(eyre::eyre!("database migration {version} was partially applied").into());
    }
    let applied = conn
        .list_applied_migrations(&MIGRATOR.table_name)
        .await
        .map_err(eyre::Report::new)?;
    let applied = applied.iter().map(|m| m.version).max().unwrap_or(0);
    let expected = MIGRATOR.iter().map(|m| m.version).max().unwrap_or(0);
    let remedy = match applied.cmp(&expected) {
        std::cmp::Ordering::Equal => return Ok(()),
        std::cmp::Ordering::Less if migrate => return Ok(()),
        std::cmp::Ordering::Less => "migrate the database",
        std::cmp::Ordering::Greater => "update the application",
    };
    Err(eyre::eyre!(
        "database is version {applied} but this binary expects version {expected} ({remedy})"
    )
    .into())
}
