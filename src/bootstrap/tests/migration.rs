//! Tests for bootstrap JSON migration and upsert helpers.

use crate::config::dotenv_layers::DotenvLayers;
use crate::test_support::ExpectValid;
use std::collections::HashMap;

use tempfile::tempdir;

use super::super::*;

fn parse_bootstrap_env(env_path: &std::path::Path) -> HashMap<String, String> {
    let bytes = ambient_fs::read(env_path).expect_valid("read bootstrap dotenv file");
    let base_dir = env_path.parent().expect_valid("dotenv file has a parent");
    DotenvLayers::from_contents(None, Some(&bytes), base_dir)
        .bootstrap_values()
        .clone()
}

fn assert_bootstrap_env_written(env_path: &std::path::Path, expected_url: &str) {
    assert!(env_path.exists(), ".env must exist after migration");
    let content = ambient_fs::read_to_string(env_path).expect_valid("read migrated .env");
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
    let dir = tempdir().expect_valid("create temp dir for bootstrap migration");
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
        serde_json::to_string_pretty(&bootstrap_json).expect_valid("serialize bootstrap.json"),
    )
    .expect_valid("write bootstrap.json");

    assert!(!env_path.exists());
    assert!(bootstrap_path.exists());

    migrate_bootstrap_json_to_env(&env_path);

    assert_bootstrap_env_written(&env_path, "postgres://localhost/axinite_upgrade");
    assert_bootstrap_file_renamed(dir.path());
}

#[test]
fn dotenv_layers_migrate_bootstrap_json_to_env() {
    let dir = tempdir().expect_valid("create temp dir for load_axinite_env migration");
    let bootstrap_path = dir.path().join("bootstrap.json");
    let bootstrap_json = serde_json::json!({
        "database_url": "postgres://localhost/axinite_public_boundary"
    });
    ambient_fs::write(
        &bootstrap_path,
        serde_json::to_string_pretty(&bootstrap_json).expect_valid("serialize bootstrap.json"),
    )
    .expect_valid("write bootstrap.json");

    let process = HashMap::from([(
        String::from("AXINITE_BASE_DIR"),
        dir.path().to_string_lossy().into_owned(),
    )]);
    let layers = DotenvLayers::load(dir.path(), &process, None);

    let env_path = dir.path().join(".env");
    assert_bootstrap_env_written(&env_path, "postgres://localhost/axinite_public_boundary");
    assert_bootstrap_file_renamed(dir.path());
    assert_eq!(
        layers
            .bootstrap_values()
            .get("DATABASE_URL")
            .map(String::as_str),
        Some("postgres://localhost/axinite_public_boundary")
    );
}

#[test]
fn test_migrate_bootstrap_json_no_database_url() {
    let dir = tempdir().expect_valid("create temp dir for no-database-url migration");
    let env_path = dir.path().join(".env");
    let bootstrap_path = dir.path().join("bootstrap.json");
    let bootstrap_json = serde_json::json!({ "onboard_completed": false });

    ambient_fs::write(
        &bootstrap_path,
        serde_json::to_string_pretty(&bootstrap_json)
            .expect_valid("serialize bootstrap without url"),
    )
    .expect_valid("write bootstrap without url");

    migrate_bootstrap_json_to_env(&env_path);

    assert!(!env_path.exists());
    assert!(bootstrap_path.exists());
}

#[test]
fn test_migrate_bootstrap_json_missing() {
    let dir = tempdir().expect_valid("create temp dir for missing bootstrap migration");
    let env_path = dir.path().join(".env");

    migrate_bootstrap_json_to_env(&env_path);

    assert!(!env_path.exists());
}

#[test]
fn upsert_bootstrap_vars_creates_file_if_missing() {
    let dir = tempdir().expect_valid("create temp dir for missing-file upsert");
    let env_path = dir.path().join("subdir").join(".env");

    assert!(!env_path.exists());

    let vars = [("DATABASE_BACKEND", "libsql")];
    upsert_bootstrap_vars_to(&env_path, &vars).expect_valid("upsert vars into missing file");

    assert!(env_path.exists());
    let parsed = parse_bootstrap_env(&env_path);
    assert_eq!(parsed.len(), 1);
    assert_eq!(
        parsed.get("DATABASE_BACKEND").map(String::as_str),
        Some("libsql")
    );
}

#[test]
fn migrate_bootstrap_json_to_env_rename_failure_leaves_env_written() {
    // Verify that when the rename of bootstrap.json fails (e.g. because
    // bootstrap.json is absent after a previous partial run), the .env
    // file that was already written is NOT removed - the rename is
    // best-effort and its failure must not undo the env-write.
    let dir = tempdir().expect_valid("create temp dir for rename-failure migration");
    let env_path = dir.path().join(".env");
    let bootstrap_path = dir.path().join("bootstrap.json");
    let bootstrap_json = serde_json::json!({
        "database_url": "postgres://localhost/axinite_rename_fail",
    });

    ambient_fs::write(
        &bootstrap_path,
        serde_json::to_string_pretty(&bootstrap_json).expect_valid("serialize"),
    )
    .expect_valid("write bootstrap.json");

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
