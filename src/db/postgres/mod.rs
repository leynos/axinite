//! PostgreSQL backend for the Database trait.
//!
//! Delegates to the existing `Store` (history) and `Repository` (workspace)
//! implementations, avoiding SQL duplication.

mod conversation;
mod job;
mod routine;
mod sandbox;
mod settings;
mod tool_failure;
mod workspace;

use deadpool_postgres::Pool;

use crate::config::DatabaseConfig;
use crate::db::NativeDatabase;
use crate::error::DatabaseError;
use crate::history::Store;
use crate::workspace::Repository;

/// PostgreSQL database backend.
///
/// Wraps the existing `Store` (for history/conversations/jobs/routines/settings)
/// and `Repository` (for workspace documents/chunks/search) to implement the
/// unified `Database` trait.
pub struct PgBackend {
    store: Store,
    repo: Repository,
}

impl PgBackend {
    /// Create a new PostgreSQL backend from configuration.
    pub async fn new(config: &DatabaseConfig) -> Result<Self, DatabaseError> {
        let store = Store::new(config).await?;
        let repo = Repository::new(store.pool());
        Ok(Self { store, repo })
    }

    /// Create a PostgreSQL backend from an existing connection pool.
    ///
    /// This is useful when the pool has already been created and initialized
    /// (e.g., during setup wizard flows).
    pub fn from_pool(pool: Pool) -> Self {
        let store = Store::from_pool(pool);
        let repo = Repository::new(store.pool());
        Self { store, repo }
    }

    /// Get a clone of the connection pool.
    ///
    /// Useful for sharing with components that still need raw pool access.
    pub(crate) fn pool(&self) -> Pool {
        self.store.pool()
    }
}

impl NativeDatabase for PgBackend {
    async fn persist_terminal_result_and_status(
        &self,
        params: crate::db::TerminalJobPersistence<'_>,
    ) -> Result<(), DatabaseError> {
        self.store.persist_terminal_result_and_status(params).await
    }

    async fn run_migrations(&self) -> Result<(), DatabaseError> {
        self.store.run_migrations().await
    }
}
