//! libSQL/Turso backend for the Database trait.
//!
//! Provides an embedded SQLite-compatible database using Turso's libSQL fork.
//! Supports three modes:
//! - Local embedded (file-based, no server needed)
//! - Turso cloud with embedded replica (sync to cloud)
//! - Temp-file-backed (for testing) — creates a UUID-named `.db` file in the
//!   OS temp directory; fresh connections share state via the file; the file
//!   and its WAL/SHM sidecars are deleted automatically when the final shared
//!   [`LibSqlDatabase`] handle is dropped. Clones returned by `shared_db()`
//!   can outlive the [`LibSqlBackend`], so cleanup follows the last shared
//!   handle rather than the backend wrapper.

mod conversations;
pub(crate) mod helpers;
mod jobs;
mod routines;
pub(crate) mod row_conversion;
mod sandbox;
mod settings;
mod tool_failures;
mod workspace;

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use crate::db::NativeDatabase;
use crate::error::DatabaseError;
use libsql::{Connection, Database as RawLibSqlDatabase};
use uuid::Uuid;

use crate::db::libsql_migrations;
pub(crate) use helpers::{
    fmt_opt_ts, fmt_ts, get_i64, get_json, get_opt_bool, get_opt_text, get_opt_ts, get_text,
    get_ts, opt_text, opt_text_owned, parse_job_state,
};
pub(crate) use row_conversion::row_to_memory_document;

/// Shared libSQL database handle.
///
/// Wraps the underlying [`RawLibSqlDatabase`] plus optional temp-file metadata
/// used by test-only temp-file-backed databases. Stores such as
/// `LibSqlSecretsStore`, `LibSqlWasmChannelStore`, and `LibSqlWasmToolStore`
/// share this handle via `Arc` so they all create connections against the same
/// underlying database and so temp-file cleanup runs when the last shared owner
/// is dropped.
pub struct LibSqlDatabase {
    db: RawLibSqlDatabase,
    /// Path to the ephemeral database file created by
    /// [`LibSqlBackend::new_memory`].
    /// `None` for persistent (`new_local` / `new_remote_replica`) backends.
    /// When `Some`, the file and its `-wal`/`-shm` sidecars are removed in
    /// [`Drop`].
    temp_path: Option<PathBuf>,
}

impl LibSqlDatabase {
    fn new(db: RawLibSqlDatabase, temp_path: Option<PathBuf>) -> Self {
        Self { db, temp_path }
    }

    #[cfg(test)]
    pub fn temp_path(&self) -> Option<PathBuf> {
        self.temp_path.clone()
    }

    /// Create a fresh libSQL connection from the shared database handle.
    ///
    /// Applies the same retry and `busy_timeout` setup used by
    /// [`LibSqlBackend::connect`] so all shared-handle consumers behave
    /// consistently.
    pub async fn connect(&self) -> Result<Connection, DatabaseError> {
        let mut last_err = None;
        for attempt in 0..3u32 {
            match self.db.connect() {
                Ok(conn) => {
                    conn.query("PRAGMA busy_timeout = 5000", ())
                        .await
                        .map_err(|e| {
                            DatabaseError::Pool(format!("Failed to set busy_timeout: {}", e))
                        })?;
                    return Ok(conn);
                }
                Err(e) => {
                    last_err = Some(e);
                    if attempt < 2 {
                        tokio::time::sleep(std::time::Duration::from_millis(
                            50 * 2u64.pow(attempt),
                        ))
                        .await;
                    }
                }
            }
        }
        Err(DatabaseError::Pool(format!(
            "Failed to create connection after 3 attempts: {}",
            last_err.map(|e| e.to_string()).unwrap_or_default()
        )))
    }
}

impl Drop for LibSqlDatabase {
    fn drop(&mut self) {
        if let Some(path) = &self.temp_path {
            let _ = ambient_fs::remove_file(path);
            let _ = ambient_fs::remove_file(path.with_extension("db-wal"));
            let _ = ambient_fs::remove_file(path.with_extension("db-shm"));
        }
    }
}

/// libSQL/Turso backend implementation of [`NativeDatabase`].
///
/// Owns one shared [`LibSqlDatabase`] handle and exposes constructors for the
/// local, remote-replica, and temp-file-backed test modes. Callers that need
/// backend-specific sharing can clone the underlying handle with
/// [`LibSqlBackend::shared_db`], while normal database operations go through
/// [`LibSqlBackend::connect`] and the trait implementations on this type.
pub struct LibSqlBackend {
    db: Arc<LibSqlDatabase>,
}

impl LibSqlBackend {
    fn ensure_parent_dir(path: &Path) -> Result<(), DatabaseError> {
        if let Some(parent) = path.parent() {
            ambient_fs::create_dir_all(parent).map_err(|e| {
                DatabaseError::Pool(format!("Failed to create database directory: {}", e))
            })?;
        }

        Ok(())
    }

    /// Wraps a built `libsql::Database` in `Self` with no temp-file path.
    fn from_db(db: RawLibSqlDatabase) -> Self {
        Self {
            db: Arc::new(LibSqlDatabase::new(db, None)),
        }
    }

    /// Create a new local embedded database.
    pub async fn new_local(path: &Path) -> Result<Self, DatabaseError> {
        Self::ensure_parent_dir(path)?;

        let db = libsql::Builder::new_local(path)
            .build()
            .await
            .map_err(|e| DatabaseError::Pool(format!("Failed to open libSQL database: {}", e)))?;

        Ok(Self::from_db(db))
    }

    /// Create a temp-file-backed database for testing.
    ///
    /// Creates a UUID-named `.db` file in [`std::env::temp_dir`]. Multiple
    /// calls to [`Self::connect`] share state through that file, matching the
    /// behaviour of the production `new_local` path without requiring a
    /// caller-supplied path.
    ///
    /// The file and its `-wal`/`-shm` sidecars are removed automatically when
    /// the final shared database handle created for this backend is dropped.
    pub async fn new_memory() -> Result<Self, DatabaseError> {
        let temp_path =
            std::env::temp_dir().join(format!("axinite-libsql-memory-{}.db", Uuid::new_v4()));
        let db = libsql::Builder::new_local(&temp_path)
            .build()
            .await
            .map_err(|e| {
                DatabaseError::Pool(format!("Failed to create temp-file-backed database: {}", e))
            })?;

        Ok(Self {
            db: Arc::new(LibSqlDatabase::new(db, Some(temp_path))),
        })
    }

    /// Create with Turso cloud sync (embedded replica).
    pub async fn new_remote_replica(
        path: &Path,
        url: &str,
        auth_token: &str,
    ) -> Result<Self, DatabaseError> {
        Self::ensure_parent_dir(path)?;

        let db = libsql::Builder::new_remote_replica(path, url.to_string(), auth_token.to_string())
            .build()
            .await
            .map_err(|e| DatabaseError::Pool(format!("Failed to open remote replica: {}", e)))?;

        Ok(Self::from_db(db))
    }

    /// Get a shared reference to the underlying database handle.
    ///
    /// Use this to pass the database to stores (SecretsStore, WasmToolStore)
    /// that need to create their own connections per-operation.
    pub fn shared_db(&self) -> Arc<LibSqlDatabase> {
        Arc::clone(&self.db)
    }

    /// Create a new connection to the database.
    ///
    /// Sets `PRAGMA busy_timeout = 5000` on every connection so concurrent
    /// writers wait up to 5 seconds instead of failing instantly with
    /// "database is locked".
    ///
    /// Retries up to 3 times with exponential backoff to handle transient
    /// "unable to open database file" errors from concurrent connection
    /// creation (e.g. cron ticker vs main thread).
    pub async fn connect(&self) -> Result<Connection, DatabaseError> {
        self.db.connect().await
    }
}

impl NativeDatabase for LibSqlBackend {
    async fn persist_terminal_result_and_status(
        &self,
        params: crate::db::TerminalJobPersistence<'_>,
    ) -> Result<(), DatabaseError> {
        LibSqlBackend::persist_terminal_result_and_status(self, params).await
    }

    async fn run_migrations(&self) -> Result<(), DatabaseError> {
        let conn = self.connect().await?;
        // WAL mode persists in the database file: all future connections benefit.
        // Readers no longer block writers and vice versa.
        conn.query("PRAGMA journal_mode=WAL", ())
            .await
            .map_err(|e| DatabaseError::Migration(format!("Failed to enable WAL mode: {}", e)))?;
        let tx = conn.transaction().await.map_err(|e| {
            DatabaseError::Migration(format!(
                "Failed to start bootstrap schema transaction: {}",
                e
            ))
        })?;
        tx.execute_batch(libsql_migrations::SCHEMA)
            .await
            .map_err(|e| DatabaseError::Migration(format!("libSQL migration failed: {}", e)))?;
        tx.commit().await.map_err(|e| {
            DatabaseError::Migration(format!(
                "Failed to commit bootstrap schema transaction: {}",
                e
            ))
        })?;
        // Apply incremental migrations (V9+) tracked in _migrations table.
        libsql_migrations::run_incremental(&conn).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
