//! Unit tests for hygiene configuration and report behaviour.

use std::sync::Mutex;

use crate::workspace::hygiene::*;

/// Serialize tests that touch the global `RUNNING` AtomicBool so they
/// don't interfere with each other when `cargo test` runs in parallel.
static RUNNING_TESTS: Mutex<()> = Mutex::new(());

#[test]
fn default_config_is_reasonable() {
    let cfg = HygieneConfig::default();
    assert!(cfg.enabled);
    assert_eq!(cfg.daily_retention_days, 30);
    assert_eq!(cfg.conversation_retention_days, 7);
    assert_eq!(cfg.cadence_hours, 12);
}

#[test]
fn report_defaults_to_no_work() {
    let report = HygieneReport::default();
    assert!(!report.had_work());
    assert!(!report.skipped);
}

#[test]
fn report_had_work_when_deleted() {
    let report = HygieneReport {
        daily_logs_deleted: 3,
        conversation_docs_deleted: 0,
        skipped: false,
    };
    assert!(report.had_work());
}

#[test]
fn report_had_work_when_conversation_deleted() {
    let report = HygieneReport {
        daily_logs_deleted: 0,
        conversation_docs_deleted: 2,
        skipped: false,
    };
    assert!(report.had_work());
}

#[test]
fn is_identity_path_excludes_sacred_docs() {
    for name in [
        "MEMORY.md",
        "IDENTITY.md",
        "SOUL.md",
        "AGENTS.md",
        "USER.md",
        "HEARTBEAT.md",
        "README.md",
        "TOOLS.md",
        "BOOTSTRAP.md",
    ] {
        assert!(is_identity_path(name), "{name} should be excluded");
        assert!(
            is_identity_path(&format!("conversations/{name}")),
            "conversations/{name} should be excluded via path"
        );
    }
}

#[test]
fn is_identity_path_case_insensitive() {
    // Verify case-insensitive matching for case-insensitive filesystems
    assert!(
        is_identity_path("memory.md"),
        "lowercase memory.md should be excluded"
    );
    assert!(
        is_identity_path("Memory.md"),
        "mixed case Memory.md should be excluded"
    );
    assert!(
        is_identity_path("MEMORY.MD"),
        "uppercase MEMORY.MD should be excluded"
    );
    assert!(
        is_identity_path("identity.md"),
        "lowercase identity.md should be excluded"
    );
    assert!(
        is_identity_path("conversations/soul.md"),
        "conversations/soul.md should be excluded"
    );
    assert!(
        is_identity_path("conversations/SOUL.MD"),
        "conversations/SOUL.MD should be excluded"
    );
}

#[test]
fn is_identity_path_allows_normal_docs() {
    for path in [
        "daily/2024-01-01.md",
        "conversations/chat-abc.md",
        "notes.md",
    ] {
        assert!(!is_identity_path(path), "{path} should not be excluded");
    }
}

#[test]
fn load_state_returns_none_for_missing_file() {
    assert!(load_state(std::path::Path::new("/tmp/nonexistent_hygiene.json")).is_none());
}

#[test]
fn save_and_load_state_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hygiene_state.json");

    save_state(&path);
    let state = load_state(&path).expect("state should be loadable after save");

    // Should be within the last second
    let elapsed = Utc::now().signed_duration_since(state.last_run);
    assert!(elapsed.num_seconds() < 2);
}

#[test]
fn save_state_creates_parent_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("deep").join("state.json");

    save_state(&path);
    assert!(path.exists());
}

#[test]
fn save_state_is_atomic_no_tmp_left_behind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    let tmp = dir.path().join("state.json.tmp");

    save_state(&path);
    assert!(path.exists(), "state file should exist");
    assert!(!tmp.exists(), "temp file should be cleaned up after rename");

    // Verify the content is valid JSON
    let state = load_state(&path).expect("saved state should be loadable");
    let elapsed = Utc::now().signed_duration_since(state.last_run);
    assert!(elapsed.num_seconds() < 2);
}

/// Regression test for issue #495: concurrent hygiene passes should be
/// serialized by the AtomicBool guard.
#[test]
fn running_guard_prevents_reentry() {
    let _lock = RUNNING_TESTS.lock().unwrap();

    // Reset the global flag to ensure a clean state
    RUNNING.store(false, Ordering::SeqCst);

    // Simulate acquiring the guard
    assert!(
        RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok(),
        "first acquisition should succeed"
    );

    // Second acquisition should fail
    assert!(
        RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err(),
        "second acquisition should fail while first is held"
    );

    // Release
    RUNNING.store(false, Ordering::SeqCst);

    // Now it should succeed again
    assert!(
        RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok(),
        "acquisition should succeed after release"
    );
    RUNNING.store(false, Ordering::SeqCst);
}

// ================================================================
// Async integration tests (require libsql backend)
// ================================================================

#[cfg(feature = "libsql")]
mod async_tests {
    //! Database-backed tests for hygiene cleanup of logs and documents.

    use super::*;
    use crate::db::Database;
    use std::sync::Arc;

    /// Helper to create a test database with migrations.
    async fn create_test_db() -> anyhow::Result<(Arc<dyn crate::db::Database>, tempfile::TempDir)> {
        use anyhow::Context as _;

        use crate::db::libsql::LibSqlBackend;

        let temp_dir = tempfile::tempdir().context("tempdir")?;
        let db_path = temp_dir.path().join("test_hygiene.db");
        let backend = LibSqlBackend::new_local(&db_path)
            .await
            .context("LibSqlBackend::new_local")?;
        backend.run_migrations().await.context("run_migrations")?;
        let db: Arc<dyn Database> = Arc::new(backend);
        Ok((db, temp_dir))
    }

    /// Helper to create a workspace from a test database.
    fn create_workspace(db: &Arc<dyn Database>) -> Arc<Workspace> {
        Arc::new(Workspace::new_with_db("default", db.clone()))
    }

    #[tokio::test]
    async fn cleanup_daily_logs_preserves_identity_documents() {
        let (db, _tmp) = create_test_db()
            .await
            .expect("create_test_db should succeed");
        let ws = create_workspace(&db);

        // Write several regular documents (non-identity)
        ws.write("daily/2024-01-15.md", "Old log")
            .await
            .expect("write log 1");
        ws.write("daily/2024-01-20.md", "Another log")
            .await
            .expect("write log 2");

        // Write an identity document
        ws.write("MEMORY.md", "Long-term curated memory")
            .await
            .expect("write identity");

        // List before cleanup
        let before = ws.list("daily/").await.expect("list before");
        let daily_count_before = before.iter().filter(|e| !e.is_directory).count();
        assert!(daily_count_before >= 2, "should have at least 2 daily logs");

        // Run cleanup with 0-day retention (deletes everything old)
        // This tests that even with aggressive cleanup, identity docs survive
        let deleted = cleanup_daily_logs(&ws, 0)
            .await
            .expect("cleanup_daily_logs");

        // Should have deleted some documents (the daily logs)
        assert!(deleted > 0, "should have deleted old daily documents");

        // Verify identity doc still exists
        let identity = db
            .get_document_by_path("default", None, "MEMORY.md")
            .await
            .expect("get identity doc");
        assert_eq!(identity.path, "MEMORY.md");
        assert_eq!(identity.content, "Long-term curated memory");
    }

    #[tokio::test]
    async fn cleanup_conversation_docs_handles_empty_directory() {
        let (db, _tmp) = create_test_db()
            .await
            .expect("create_test_db should succeed");
        let ws = create_workspace(&db);

        // Run cleanup on an empty directory (conversations/ doesn't exist)
        let deleted = cleanup_conversation_docs(&ws, 7)
            .await
            .expect("cleanup_conversation_docs");

        // Should delete 0 (nothing to delete)
        assert_eq!(deleted, 0, "should delete 0 from empty directory");
    }

    #[tokio::test]
    async fn cleanup_respects_cadence_prevents_concurrent_runs() {
        let (db, _tmp) = create_test_db()
            .await
            .expect("create_test_db should succeed");
        let ws = create_workspace(&db);

        let config = HygieneConfig {
            enabled: true,
            daily_retention_days: 30,
            conversation_retention_days: 7,
            cadence_hours: 12,
            state_dir: _tmp.path().to_path_buf(),
        };

        // First run should succeed
        let report1 = run_if_due(&ws, &config).await;
        assert!(!report1.skipped, "first run should not be skipped");

        // Second run immediately should be skipped (cadence not elapsed)
        let report2 = run_if_due(&ws, &config).await;
        assert!(report2.skipped, "second run should be skipped by cadence");

        // Report structure should be correct
        assert_eq!(
            report1.daily_logs_deleted + report1.conversation_docs_deleted,
            0,
            "first run should have clean counts"
        );
    }

    #[tokio::test]
    async fn cleanup_reports_deletion_counts_correctly() {
        let (db, _tmp) = create_test_db()
            .await
            .expect("create_test_db should succeed");
        let ws = create_workspace(&db);

        // Write some documents
        ws.write("daily/log1.md", "content 1")
            .await
            .expect("write doc 1");
        ws.write("daily/log2.md", "content 2")
            .await
            .expect("write doc 2");
        ws.write("conversations/chat1.md", "content 3")
            .await
            .expect("write doc 3");

        // Run with 0-day retention to delete everything non-identity
        let deleted_daily = cleanup_daily_logs(&ws, 0).await.expect("cleanup daily");
        let deleted_conv = cleanup_conversation_docs(&ws, 0)
            .await
            .expect("cleanup conversations");

        // Both should report deletions
        assert!(deleted_daily > 0, "should report deleted daily logs");
        assert_eq!(deleted_conv, 1, "should report 1 deleted conversation doc");

        // Create a HygieneReport and verify aggregation works
        let report = HygieneReport {
            daily_logs_deleted: deleted_daily,
            conversation_docs_deleted: deleted_conv,
            skipped: false,
        };

        // Verify HygieneReport structure
        assert!(!report.skipped, "should not be skipped");
        assert!(report.had_work(), "report should indicate work was done");
        assert!(
            report.daily_logs_deleted > 0 || report.conversation_docs_deleted > 0,
            "report should have at least one deletion count > 0"
        );

        // Verify had_work() correctly combines both counts
        let no_work = HygieneReport {
            daily_logs_deleted: 0,
            conversation_docs_deleted: 0,
            skipped: false,
        };
        assert!(!no_work.had_work(), "empty report should indicate no work");
    }
}
