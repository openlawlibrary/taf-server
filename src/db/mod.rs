//! Database related module.
use async_trait::async_trait;
use sqlx::Transaction;
use std::str::FromStr as _;
use std::time;

use std::thread::available_parallelism;

use sqlx::any::{self, AnyPoolOptions};
use sqlx::AnyPool;
use sqlx::ConnectOptions as _;
use tracing::{instrument, log};

/// Database initialization.
pub mod init;
/// Models for the database.
pub mod models;

#[async_trait]
/// Generic Database
pub trait Db {
    /// Connects to a database.
    ///
    /// # Errors
    /// Errors if connection to database fails.
    async fn connect(url: &str) -> anyhow::Result<DatabaseConnection>;
}

#[async_trait]
/// Generic transaction
pub trait Tx {
    /// Begin a transaction.
    async fn begin(pool: AnyPool) -> anyhow::Result<DatabaseTransaction>;
    /// Commit a transaction.
    async fn commit(self) -> anyhow::Result<()>;
    /// Rollback a transaction.
    async fn rollback(self) -> anyhow::Result<()>;
}

/// Type of database connection.
#[derive(Debug, Clone)]
pub enum DatabaseKind {
    /// Sqlite database.
    Sqlite,
}

/// Database connection.
#[derive(Debug, Clone)]
pub struct DatabaseConnection {
    /// Database connection pool.
    pub pool: AnyPool,
    /// Type of database connection.
    pub kind: DatabaseKind,
}

/// Database transaction.
pub struct DatabaseTransaction {
    /// Database transaction.
    pub tx: Transaction<'static, sqlx::Any>,
}

impl DatabaseConnection {
    /// Connects to a database, applying the given `SQLite` journal mode.
    ///
    /// # Errors
    /// Errors if connection to database fails.
    pub async fn connect_with_journal_mode(
        db_url: &str,
        journal_mode: &str,
    ) -> anyhow::Result<Self> {
        any::install_default_drivers();
        let num_cpus = match available_parallelism() {
            Ok(cpus) => u32::try_from(cpus.get())?,
            Err(_) => 4,
        };
        let options = any::AnyConnectOptions::from_str(db_url)?
            .log_slow_statements(log::LevelFilter::Debug, time::Duration::from_secs(1));
        let db_kind = if db_url.starts_with("sqlite:///") {
            DatabaseKind::Sqlite
        } else {
            anyhow::bail!("Unsupported database URL: {db_url}");
        };
        let pool = AnyPoolOptions::new()
            .max_connections(2 * num_cpus)
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    // Enable foreign key enforcement on every connection.
                    // This is a per-connection SQLite setting (not persisted to the file),
                    // so it must be applied here rather than in a one-off PRAGMA query.
                    sqlx::query("PRAGMA foreign_keys = ON")
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect_with(options)
            .await?;
        let connection = Self {
            pool,
            kind: db_kind,
        };
        // Journal mode is a database-level setting (persisted to the file header), so a
        // single PRAGMA query is sufficient.
        // `journal_mode` is always a hardcoded, internally-controlled literal (never derived
        // from user/request input), so it's safe to assert as SQL-safe here.
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "PRAGMA journal_mode={journal_mode};"
        )))
        .execute(&connection.pool)
        .await?;

        Ok(connection)
    }
}

#[async_trait]
impl Db for DatabaseConnection {
    /// Connects to a database.
    ///
    /// # Errors
    /// Errors if connection to database fails.
    #[instrument(level = "trace")]
    async fn connect(db_url: &str) -> anyhow::Result<Self> {
        Self::connect_with_journal_mode(db_url, "WAL").await
    }
}

#[async_trait]
impl Tx for DatabaseTransaction {
    /// Begin a transaction.
    async fn begin(pool: AnyPool) -> anyhow::Result<Self> {
        let tx = pool.begin().await?;
        Ok(Self { tx })
    }
    /// Commit a transaction.
    async fn commit(self) -> anyhow::Result<()> {
        self.tx.commit().await?;
        Ok(())
    }

    /// Rollback a transaction.
    async fn rollback(self) -> anyhow::Result<()> {
        self.tx.rollback().await?;
        Ok(())
    }
}
