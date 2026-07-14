//! Unit tests for the libSQL database backend and timestamp parsing.

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
    ambient_fs::write(&path, b"").expect("failed to create temp db file");
    ambient_fs::write(&wal, b"").expect("failed to create sidecar file");
    ambient_fs::write(&shm, b"").expect("failed to create sidecar file");

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
