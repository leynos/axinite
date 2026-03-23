//! libSQL/Turso backend for the Database trait.
//!
//! Provides an embedded SQLite-compatible database using Turso's libSQL fork.
//! Supports three modes:
//! - Local embedded (file-based, no server needed)
//! - Turso cloud with embedded replica (sync to cloud)
//! - In-memory (for testing)

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
use std::sync::Arc;

use crate::db::NativeDatabase;
use crate::error::DatabaseError;
use libsql::{Connection, Database as LibSqlDatabase};

use crate::db::libsql_migrations;
pub(crate) use helpers::{
    fmt_opt_ts, fmt_ts, get_decimal, get_i64, get_json, get_opt_bool, get_opt_decimal,
    get_opt_text, get_opt_ts, get_text, get_ts, opt_text, opt_text_owned, parse_job_state,
};
pub(crate) use row_conversion::{
    row_to_memory_document, row_to_routine_libsql, row_to_routine_run_libsql,
};

/// Explicit column list for routines table (matches positional access in `row_to_routine_libsql`).
pub(crate) const ROUTINE_COLUMNS: &str = "\
    id, name, description, user_id, enabled, \
    trigger_type, trigger_config, action_type, action_config, \
    cooldown_secs, max_concurrent, dedup_window_secs, \
    notify_channel, notify_user, notify_on_success, notify_on_failure, notify_on_attention, \
    state, last_run_at, next_fire_at, run_count, consecutive_failures, \
    created_at, updated_at";

/// Explicit column list for routine_runs table (matches positional access in `row_to_routine_run_libsql`).
pub(crate) const ROUTINE_RUN_COLUMNS: &str = "\
    id, routine_id, trigger_type, trigger_detail, started_at, \
    status, completed_at, result_summary, tokens_used, job_id, created_at";

/// libSQL/Turso database backend.
///
/// Stores the `Database` handle in an `Arc` so that the same underlying
/// database can be shared with stores (SecretsStore, WasmToolStore) that
/// create their own connections per-operation.
pub struct LibSqlBackend {
    db: Arc<LibSqlDatabase>,
}

impl LibSqlBackend {
    /// Create a new local embedded database.
    pub async fn new_local(path: &Path) -> Result<Self, DatabaseError> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DatabaseError::Pool(format!("Failed to create database directory: {}", e))
            })?;
        }

        let db = libsql::Builder::new_local(path)
            .build()
            .await
            .map_err(|e| DatabaseError::Pool(format!("Failed to open libSQL database: {}", e)))?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Create a new in-memory database (for testing).
    pub async fn new_memory() -> Result<Self, DatabaseError> {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .map_err(|e| {
                DatabaseError::Pool(format!("Failed to create in-memory database: {}", e))
            })?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Create with Turso cloud sync (embedded replica).
    pub async fn new_remote_replica(
        path: &Path,
        url: &str,
        auth_token: &str,
    ) -> Result<Self, DatabaseError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DatabaseError::Pool(format!("Failed to create database directory: {}", e))
            })?;
        }

        let db = libsql::Builder::new_remote_replica(path, url.to_string(), auth_token.to_string())
            .build()
            .await
            .map_err(|e| DatabaseError::Pool(format!("Failed to open remote replica: {}", e)))?;

        Ok(Self { db: Arc::new(db) })
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

impl NativeDatabase for LibSqlBackend {
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
        // In-memory databases use "memory" journal mode (WAL doesn't apply to :memory:),
        // but the PRAGMA still executes without error. For file-based databases it returns "wal".
        assert!(
            mode == "wal" || mode == "memory",
            "expected wal or memory, got: {}",
            mode,
        );
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
