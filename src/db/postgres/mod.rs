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

    /// Get a clone of the connection pool.
    ///
    /// Useful for sharing with components that still need raw pool access.
    pub(crate) fn pool(&self) -> Pool {
        self.store.pool()
    }
}

impl NativeDatabase for PgBackend {
    async fn run_migrations(&self) -> Result<(), DatabaseError> {
        self.store.run_migrations().await
    }
}
