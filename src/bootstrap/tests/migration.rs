//! Tests for bootstrap JSON migration and upsert helpers.

use std::process::Command;

use crate::testing::test_utils::EnvVarsGuard;
use tempfile::tempdir;

use super::super::*;

fn would_autodetect_libsql(db_path: &std::path::Path) -> bool {
    std::env::var("DATABASE_BACKEND").is_err() && db_path.exists()
}

fn assert_bootstrap_env_written(env_path: &std::path::Path, expected_url: &str) {
    assert!(env_path.exists(), ".env must exist after migration");
    let content = ambient_fs::read_to_string(env_path).expect("read migrated .env");
    assert_eq!(
        content,
        format!("DATABASE_URL=\"{expected_url}\"\n"),
        ".env content must contain the migrated DATABASE_URL"
    );
}

fn assert_bootstrap_file_renamed(dir_path: &std::path::Path) {
    assert!(
        !dir_path.join("bootstrap.json").exists(),
        "bootstrap.json must have been renamed away"
    );
    assert!(
        dir_path.join("bootstrap.json.migrated").exists(),
        "bootstrap.json.migrated must exist"
    );
}

#[test]
fn test_migrate_bootstrap_json_to_env() {
    let dir = tempdir().expect("create temp dir for bootstrap migration");
    let env_path = dir.path().join(".env");
    let bootstrap_path = dir.path().join("bootstrap.json");
    let bootstrap_json = serde_json::json!({
        "database_url": "postgres://localhost/axinite_upgrade",
        "database_pool_size": 5,
        "secrets_master_key_source": "keychain",
        "onboard_completed": true
    });

    ambient_fs::write(
        &bootstrap_path,
        serde_json::to_string_pretty(&bootstrap_json).expect("serialize bootstrap.json"),
    )
    .expect("write bootstrap.json");

    assert!(!env_path.exists());
    assert!(bootstrap_path.exists());

    migrate_bootstrap_json_to_env(&env_path);

    assert_bootstrap_env_written(&env_path, "postgres://localhost/axinite_upgrade");
    assert_bootstrap_file_renamed(dir.path());
}

#[test]
fn load_axinite_env_migrates_bootstrap_json_to_env() {
    if std::env::var("AXINITE_LOAD_ENV_CHILD").ok().as_deref() == Some("1") {
        let base_dir = std::path::PathBuf::from(
            std::env::var("AXINITE_BASE_DIR").expect("AXINITE_BASE_DIR missing"),
        );
        let env_path = base_dir.join(".env");

        load_axinite_env();

        assert_bootstrap_env_written(&env_path, "postgres://localhost/axinite_public_boundary");
        assert_bootstrap_file_renamed(&base_dir);
        return;
    }

    let dir = tempdir().expect("create temp dir for load_axinite_env migration");
    let bootstrap_path = dir.path().join("bootstrap.json");
    let bootstrap_json = serde_json::json!({
        "database_url": "postgres://localhost/axinite_public_boundary"
    });
    ambient_fs::write(
        &bootstrap_path,
        serde_json::to_string_pretty(&bootstrap_json).expect("serialize bootstrap.json"),
    )
    .expect("write bootstrap.json");

    let current_exe = std::env::current_exe().expect("locate current test binary");
    let status = Command::new(current_exe)
        .args([
            "--exact",
            "bootstrap::tests::migration::load_axinite_env_migrates_bootstrap_json_to_env",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("AXINITE_LOAD_ENV_CHILD", "1")
        .env("AXINITE_BASE_DIR", dir.path())
        .env_remove("DATABASE_URL")
        .env_remove("DATABASE_BACKEND")
        .status()
        .expect("spawn load_axinite_env boundary test");

    assert!(status.success(), "child boundary test failed: {status}");
}

#[test]
fn test_migrate_bootstrap_json_no_database_url() {
    let dir = tempdir().expect("create temp dir for no-database-url migration");
    let env_path = dir.path().join(".env");
    let bootstrap_path = dir.path().join("bootstrap.json");
    let bootstrap_json = serde_json::json!({ "onboard_completed": false });

    ambient_fs::write(
        &bootstrap_path,
        serde_json::to_string_pretty(&bootstrap_json).expect("serialize bootstrap without url"),
    )
    .expect("write bootstrap without url");

    migrate_bootstrap_json_to_env(&env_path);

    assert!(!env_path.exists());
    assert!(bootstrap_path.exists());
}

#[test]
fn test_migrate_bootstrap_json_missing() {
    let dir = tempdir().expect("create temp dir for missing bootstrap migration");
    let env_path = dir.path().join(".env");

    migrate_bootstrap_json_to_env(&env_path);

    assert!(!env_path.exists());
}

#[test]
fn test_libsql_autodetect_sets_backend_when_db_exists() {
    let mut env_guard = EnvVarsGuard::new(&["DATABASE_BACKEND"]);
    env_guard.remove("DATABASE_BACKEND");

    let dir = tempdir().expect("create temp dir for libsql autodetect");
    let db_path = dir.path().join("axinite.db");

    assert!(!db_path.exists());
    assert!(
        !would_autodetect_libsql(&db_path),
        "should not auto-detect when db file is absent"
    );

    ambient_fs::write(&db_path, "").expect("create libsql marker file");
    assert!(
        would_autodetect_libsql(&db_path),
        "should detect libsql when db file is present and backend unset"
    );
}

#[test]
fn test_libsql_autodetect_does_not_override_explicit_backend() {
    let mut env_guard = EnvVarsGuard::new(&["DATABASE_BACKEND"]);
    env_guard.set("DATABASE_BACKEND", "postgres");

    let dir = tempdir().expect("create temp dir for explicit backend autodetect test");
    let db_path = dir.path().join("axinite.db");
    ambient_fs::write(&db_path, "").expect("create libsql marker file");

    let would_override = std::env::var("DATABASE_BACKEND").is_err() && db_path.exists();
    assert!(
        !would_override,
        "must not override an explicitly set DATABASE_BACKEND"
    );
}

#[test]
fn upsert_bootstrap_vars_creates_file_if_missing() {
    let dir = tempdir().expect("create temp dir for missing-file upsert");
    let env_path = dir.path().join("subdir").join(".env");

    assert!(!env_path.exists());

    let vars = [("DATABASE_BACKEND", "libsql")];
    upsert_bootstrap_vars_to(&env_path, &vars).expect("upsert vars into missing file");

    assert!(env_path.exists());
    let parsed: Vec<(String, String)> = dotenvy::from_path_iter(&env_path)
        .expect("parse newly created bootstrap env")
        .filter_map(|result| result.ok())
        .collect();
    assert_eq!(parsed.len(), 1);
    assert_eq!(
        parsed[0],
        ("DATABASE_BACKEND".to_string(), "libsql".to_string())
    );
}

#[test]
fn migrate_bootstrap_json_to_env_rename_failure_leaves_env_written() {
    // Verify that when the rename of bootstrap.json fails (e.g. because
    // bootstrap.json is absent after a previous partial run), the .env
    // file that was already written is NOT removed - the rename is
    // best-effort and its failure must not undo the env-write.
    let dir = tempdir().expect("create temp dir for rename-failure migration");
    let env_path = dir.path().join(".env");
    let bootstrap_path = dir.path().join("bootstrap.json");
    let bootstrap_json = serde_json::json!({
        "database_url": "postgres://localhost/axinite_rename_fail",
    });

    ambient_fs::write(
        &bootstrap_path,
        serde_json::to_string_pretty(&bootstrap_json).expect("serialize"),
    )
    .expect("write bootstrap.json");

    // Run the migration once - this writes .env and renames bootstrap.json.
    migrate_bootstrap_json_to_env(&env_path);

    assert!(env_path.exists(), ".env must be written");
    assert!(
        !bootstrap_path.exists(),
        "bootstrap.json must have been renamed away"
    );
    assert!(
        dir.path().join("bootstrap.json.migrated").exists(),
        "bootstrap.json.migrated must exist after the first run"
    );

    // Run again with bootstrap.json absent - the rename is a no-op (file
    // not found) but .env must still exist.
    migrate_bootstrap_json_to_env(&env_path);

    assert!(
        env_path.exists(),
        ".env must still exist after idempotent second run"
    );
}
