//! Tests for migrating legacy disk settings into the settings store.

use tempfile::tempdir;

use crate::bootstrap::MigrationError;

use super::migration_support::{
    MigrationStore, assert_legacy_file_not_renamed, assert_legacy_file_renamed, assert_store_state,
    write_legacy_settings,
};

#[tokio::test]
async fn migrate_disk_to_db_from_dir_missing_legacy_file_is_noop() {
    let dir = tempdir().expect("create temp dir for missing settings migration");
    let store = MigrationStore::new(Ok(false));

    super::super::migration::migrate_disk_to_db_from_dir(&store, "test-user", dir.path())
        .await
        .expect("missing settings migration should succeed");

    assert_store_state(&store, 0, 0);
    assert!(!dir.path().join("settings.json.migrated").exists());
}

#[tokio::test]
async fn migrate_disk_to_db_from_dir_renames_when_db_already_has_settings() {
    let dir = tempdir().expect("create temp dir for stale settings migration");
    let _settings_path = write_legacy_settings(&dir);
    let store = MigrationStore::new(Ok(true));

    super::super::migration::migrate_disk_to_db_from_dir(&store, "test-user", dir.path())
        .await
        .expect("stale settings migration should succeed");

    assert_store_state(&store, 1, 0);
    assert_legacy_file_renamed(&dir);
}

#[tokio::test]
async fn migrate_disk_to_db_from_dir_writes_settings_and_renames_legacy_file() {
    let dir = tempdir().expect("create temp dir for settings migration");
    let _settings_path = write_legacy_settings(&dir);
    let store = MigrationStore::new(Ok(false));

    super::super::migration::migrate_disk_to_db_from_dir(&store, "test-user", dir.path())
        .await
        .expect("settings migration should succeed");

    assert_store_state(&store, 1, 1);
    assert_eq!(
        store.state().captured_settings.get("onboard_completed"),
        Some(&serde_json::Value::Bool(true))
    );
    assert_legacy_file_renamed(&dir);
}

#[tokio::test]
async fn migrate_disk_to_db_from_dir_db_failure_leaves_legacy_file_unmigrated() {
    let dir = tempdir().expect("create temp dir for failed settings migration");
    let _settings_path = write_legacy_settings(&dir);
    let store = MigrationStore::with_set_all_error();

    let error =
        super::super::migration::migrate_disk_to_db_from_dir(&store, "test-user", dir.path())
            .await
            .expect_err("database write failure should abort migration");

    assert!(
        matches!(error, MigrationError::Database(ref message) if message.contains("Failed to write settings to DB"))
    );
    assert_store_state(&store, 1, 1);
    assert_legacy_file_not_renamed(&dir);
}

#[tokio::test]
async fn migrate_disk_to_db_from_dir_is_ok_after_best_effort_rename_removed_source() {
    let dir = tempdir().expect("create temp dir for repeated settings migration");
    let _settings_path = write_legacy_settings(&dir);
    let store = MigrationStore::new(Ok(false));

    super::super::migration::migrate_disk_to_db_from_dir(&store, "test-user", dir.path())
        .await
        .expect("first settings migration should succeed");
    super::super::migration::migrate_disk_to_db_from_dir(&store, "test-user", dir.path())
        .await
        .expect("second settings migration should succeed after source was renamed");

    assert_store_state(&store, 1, 1);
    assert_legacy_file_renamed(&dir);
}
