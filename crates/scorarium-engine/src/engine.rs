use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use sqlx::pool::PoolConnection;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::{ConnectOptions, Sqlite, SqlitePool};

use crate::Result;
use crate::audit::Audited;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

#[derive(Debug, Default)]
pub struct EngineOptions {
    /// Run migrations on startup, otherwise startup on an outdated database will fail.
    ///
    /// A missing database is always created and migrated.
    pub migrate_on_startup: bool,
}

#[derive(Clone, Debug)]
pub struct Engine {
    inner: Arc<EngineInner>,
}

#[derive(Debug)]
struct EngineInner {
    pool: SqlitePool,
    _temp_dir: Option<tempfile::TempDir>,
}

impl Engine {
    /// Open the engine in `data_dir`, creating the directory and database as necessary
    pub async fn open(data_dir: impl AsRef<Path>, options: EngineOptions) -> Result<Engine> {
        let data_dir = data_dir.as_ref();
        std::fs::create_dir_all(data_dir)?;
        Engine::new(data_dir, options.migrate_on_startup, None).await
    }

    /// Open a temporary engine filled with the demo libraries
    ///
    /// The temporary directory is cleaned up on shutdown. A fresh database is always migrated, so
    /// [EngineOptions::migrate_on_startup] has no effect.
    pub async fn demo(_options: EngineOptions) -> Result<Engine> {
        let dir = tempfile::tempdir()?;
        let data_dir = dir.path().to_path_buf();
        let engine = Engine::new(&data_dir, true, Some(dir)).await?;
        crate::demo::populate(&engine).await?;
        Ok(engine)
    }

    async fn new(
        data_dir: &Path,
        migrate_on_startup: bool,
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
                _temp_dir: temp_dir,
            }),
        })
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
