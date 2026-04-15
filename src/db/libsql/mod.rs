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
            let _ = std::fs::remove_file(path);
            let _ = std::fs::remove_file(path.with_extension("db-wal"));
            let _ = std::fs::remove_file(path.with_extension("db-shm"));
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
            std::fs::create_dir_all(parent).map_err(|e| {
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
mod tests {
    use std::sync::Arc;

    use chrono::{TimeZone, Utc};

    use crate::db::Database;
    use crate::db::libsql::LibSqlBackend;
    use crate::db::libsql::helpers::parse_timestamp;

    #[test]
    fn test_parse_timestamp_accepts_rfc3339_and_legacy_naive_formats() {
        let expected = Utc.with_ymd_and_hms(2026, 3, 7, 12, 34, 56).unwrap();

        let with_millis = parse_timestamp("2026-03-07T12:34:56.789Z").unwrap();
        assert_eq!(with_millis, expected + chrono::Duration::milliseconds(789));

        let naive_with_millis = parse_timestamp("2026-03-07 12:34:56.789").unwrap();
        assert_eq!(
            naive_with_millis,
            expected + chrono::Duration::milliseconds(789)
        );

        let naive_without_millis = parse_timestamp("2026-03-07 12:34:56").unwrap();
        assert_eq!(naive_without_millis, expected);
    }

    #[tokio::test]
    async fn test_libsql_now_format_is_rfc3339_and_parseable() {
        let backend = LibSqlBackend::new_memory().await.unwrap();
        backend.run_migrations().await.unwrap();

        let conn = backend.connect().await.unwrap();
        let mut rows = conn
            .query("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", ())
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let ts: String = row.get(0).unwrap();

        let parsed = parse_timestamp(&ts).unwrap();
        assert_eq!(
            ts,
            parsed.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        );
    }

    #[tokio::test]
    async fn test_wal_mode_after_migrations() {
        let backend = LibSqlBackend::new_memory().await.unwrap();
        backend.run_migrations().await.unwrap();

        let conn = backend.connect().await.unwrap();
        let mut rows = conn.query("PRAGMA journal_mode", ()).await.unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let mode: String = row.get(0).unwrap();
        // The temp-file-backed test database should still enable WAL mode.
        assert!(mode == "wal", "expected wal, got: {}", mode,);
    }

    #[tokio::test]
    async fn test_busy_timeout_set_on_connect() {
        let backend = LibSqlBackend::new_memory().await.unwrap();
        backend.run_migrations().await.unwrap();

        let conn = backend.connect().await.unwrap();
        let mut rows = conn.query("PRAGMA busy_timeout", ()).await.unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let timeout: i64 = row.get(0).unwrap();
        assert_eq!(timeout, 5000);
    }

    #[test]
    fn shared_libsql_database_drop_removes_temp_files() {
        let runtime = tokio::runtime::Runtime::new().expect("create runtime for libsql test");
        let backend = runtime
            .block_on(LibSqlBackend::new_memory())
            .expect("failed to create temp-file-backed backend");
        let shared_db = backend.shared_db();

        let path = shared_db
            .temp_path()
            .expect("new_memory must set temp_path");
        let wal = path.with_extension("db-wal");
        let shm = path.with_extension("db-shm");

        // Touch the database and sidecar files so the drop handler has
        // something concrete to delete.
        std::fs::write(&path, b"").expect("failed to create temp db file");
        std::fs::write(&wal, b"").expect("failed to create sidecar file");
        std::fs::write(&shm, b"").expect("failed to create sidecar file");

        assert!(path.exists(), "temp db file must exist before drop");
        assert!(wal.exists(), "WAL sidecar must exist before drop");
        assert!(shm.exists(), "SHM sidecar must exist before drop");

        drop(backend);
        assert!(
            path.exists(),
            "shared handle should keep temp db file alive after backend drop"
        );

        let shared_db = match Arc::try_unwrap(shared_db) {
            Ok(shared_db) => shared_db,
            Err(_) => panic!("test should hold the final shared libsql database handle"),
        };

        drop(shared_db);

        assert!(!path.exists(), "temp db file must be removed after drop");
        assert!(!wal.exists(), "WAL sidecar must be removed after drop");
        assert!(!shm.exists(), "SHM sidecar must be removed after drop");
    }

    /// Regression test: save_job must persist user_id and get_job must return it.
    #[tokio::test]
    async fn test_save_job_persists_user_id() {
        use crate::context::JobContext;
        use crate::db::JobStore;

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_user_id.db");
        let backend = LibSqlBackend::new_local(&db_path).await.unwrap();
        backend.run_migrations().await.unwrap();

        let ctx = JobContext::with_user("test-user-42", "Test Job", "A test job");
        backend.save_job(&ctx).await.unwrap();

        let loaded = backend.get_job(ctx.job_id).await.unwrap().unwrap();
        assert_eq!(loaded.user_id, "test-user-42");
    }

    #[tokio::test]
    async fn test_concurrent_writes_succeed() {
        // Use a temp file so connections share state (in-memory DBs are connection-local)
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_concurrent.db");
        let backend = LibSqlBackend::new_local(&db_path).await.unwrap();
        backend.run_migrations().await.unwrap();

        // Spawn 20 concurrent inserts into the conversations table
        let mut handles = Vec::new();
        for i in 0..20 {
            let conn = backend.connect().await.unwrap();
            let handle = tokio::spawn(async move {
                let id = uuid::Uuid::new_v4().to_string();
                let val = format!("ch_{}", i);
                conn.execute(
                    "INSERT INTO conversations (id, channel, user_id) VALUES (?1, ?2, ?3)",
                    libsql::params![id, val, "test_user"],
                )
                .await
            });
            handles.push(handle);
        }

        for handle in handles {
            let result = handle.await.unwrap();
            assert!(
                result.is_ok(),
                "concurrent write failed: {:?}",
                result.err()
            );
        }

        // Verify all 20 rows landed
        let conn = backend.connect().await.unwrap();
        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM conversations WHERE user_id = ?1",
                libsql::params!["test_user"],
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        let count: i64 = row.get(0).unwrap();
        assert_eq!(count, 20);
    }

    #[tokio::test]
    async fn test_conversations_metadata_must_be_valid_json() {
        let backend = LibSqlBackend::new_memory()
            .await
            .expect("create in-memory libsql backend for metadata JSON validation");
        backend
            .run_migrations()
            .await
            .expect("run migrations before metadata JSON validation");

        let conn = backend
            .connect()
            .await
            .expect("connect to libsql backend for metadata JSON validation");
        let result = conn
            .execute(
                "INSERT INTO conversations (id, channel, user_id, metadata) VALUES (?1, ?2, ?3, ?4)",
                libsql::params![
                    uuid::Uuid::new_v4().to_string(),
                    "web",
                    "test-user",
                    "not-json"
                ],
            )
            .await;

        assert!(result.is_err(), "invalid JSON metadata must be rejected");
    }

    #[tokio::test]
    async fn test_connect_retry_succeeds_on_valid_db() {
        // Verify connect() works with retry logic on a file-backed DB
        // (exercises the retry path even though transient failures are hard
        // to reproduce deterministically).
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test_retry.db");
        let backend = LibSqlBackend::new_local(&db_path).await.unwrap();
        backend.run_migrations().await.unwrap();

        // Multiple concurrent connect() calls should all succeed
        let mut handles = Vec::new();
        for _ in 0..10 {
            let b = LibSqlBackend {
                db: backend.shared_db(),
            };
            handles.push(tokio::spawn(async move { b.connect().await }));
        }

        for handle in handles {
            let result = handle.await.unwrap();
            assert!(
                result.is_ok(),
                "concurrent connect failed: {:?}",
                result.err()
            );
        }
    }
}
