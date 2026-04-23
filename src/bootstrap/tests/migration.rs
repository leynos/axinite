//! Tests for bootstrap migration and upsert helpers.

use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use tempfile::tempdir;
use tracing_test::traced_test;

use crate::testing::test_utils::EnvVarsGuard;

use super::super::*;

#[test]
fn test_migrate_bootstrap_json_to_env() {
    let dir = tempdir().expect("create temp dir for bootstrap migration");
    let env_path = dir.path().join(".env");
    let bootstrap_path = dir.path().join("bootstrap.json");
    let bootstrap_json = serde_json::json!({
        "database_url": "postgres://localhost/ironclaw_upgrade",
        "database_pool_size": 5,
        "secrets_master_key_source": "keychain",
        "onboard_completed": true
    });

    std::fs::write(
        &bootstrap_path,
        serde_json::to_string_pretty(&bootstrap_json).expect("serialize bootstrap.json"),
    )
    .expect("write bootstrap.json");

    assert!(!env_path.exists());
    assert!(bootstrap_path.exists());

    migrate_bootstrap_json_to_env(&env_path);

    assert!(env_path.exists());
    let content = std::fs::read_to_string(&env_path).expect("read migrated .env");
    assert_eq!(
        content,
        "DATABASE_URL=\"postgres://localhost/ironclaw_upgrade\"\n"
    );
    assert!(!bootstrap_path.exists());
    assert!(dir.path().join("bootstrap.json.migrated").exists());
}

#[test]
fn test_migrate_bootstrap_json_no_database_url() {
    let dir = tempdir().expect("create temp dir for no-database-url migration");
    let env_path = dir.path().join(".env");
    let bootstrap_path = dir.path().join("bootstrap.json");
    let bootstrap_json = serde_json::json!({ "onboard_completed": false });

    std::fs::write(
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
    let db_path = dir.path().join("ironclaw.db");

    assert!(!db_path.exists());
    let would_trigger = std::env::var("DATABASE_BACKEND").is_err() && db_path.exists();
    assert!(
        !would_trigger,
        "should not auto-detect when db file is absent"
    );

    std::fs::write(&db_path, "").expect("create libsql marker file");
    assert!(db_path.exists());

    let detected = std::env::var("DATABASE_BACKEND").is_err() && db_path.exists();
    assert!(
        detected,
        "should detect libsql when db file is present and backend unset"
    );
}

#[test]
fn test_libsql_autodetect_does_not_override_explicit_backend() {
    let mut env_guard = EnvVarsGuard::new(&["DATABASE_BACKEND"]);
    env_guard.set("DATABASE_BACKEND", "postgres");

    let dir = tempdir().expect("create temp dir for explicit backend autodetect test");
    let db_path = dir.path().join("ironclaw.db");
    std::fs::write(&db_path, "").expect("create libsql marker file");

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
fn rename_to_migrated_success() {
    let dir = tempdir().expect("create temp dir for rename success");
    let path = dir.path().join("settings.json");
    std::fs::write(&path, "{}").expect("write legacy settings file");

    super::super::migration::rename_to_migrated(&path).expect("rename legacy settings");

    assert!(!path.exists());
    assert!(dir.path().join("settings.json.migrated").exists());
}

#[test]
#[traced_test]
fn rename_to_migrated_missing_source() {
    let dir = tempdir().expect("create temp dir for missing-file rename");
    let path = dir.path().join("missing.json");

    let error = super::super::migration::rename_to_migrated(&path)
        .expect_err("missing source should fail to rename");

    assert_eq!(error.kind(), ErrorKind::NotFound);
    assert!(logs_contain("Failed to rename"));
}

#[cfg(unix)]
#[test]
#[traced_test]
fn rename_to_migrated_permission_denied() {
    let dir = tempdir().expect("create temp dir for permission-denied rename");
    let path = dir.path().join("settings.json");
    std::fs::write(&path, "{}").expect("write legacy settings file");

    let original_dir_perms = std::fs::metadata(dir.path())
        .expect("read directory metadata")
        .permissions();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555))
        .expect("make directory read-only");

    let error = super::super::migration::rename_to_migrated(&path)
        .expect_err("read-only directory should block rename");

    std::fs::set_permissions(dir.path(), original_dir_perms)
        .expect("restore temp directory permissions");

    assert_eq!(error.kind(), ErrorKind::PermissionDenied);
    assert!(logs_contain("Failed to rename"));
}

#[cfg(unix)]
#[test]
#[traced_test]
fn rename_legacy_bootstrap_logs_success_only_on_ok() {
    let dir = tempdir().expect("create temp dir for bootstrap rename logging");
    let bootstrap_path = dir.path().join("bootstrap.json");
    std::fs::write(&bootstrap_path, "{}").expect("write bootstrap file");

    let original_dir_perms = std::fs::metadata(dir.path())
        .expect("read directory metadata")
        .permissions();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555))
        .expect("make directory read-only");

    super::super::migration::rename_legacy_bootstrap(dir.path());

    std::fs::set_permissions(dir.path(), original_dir_perms)
        .expect("restore temp directory permissions");

    assert!(logs_contain("Failed to rename"));
    assert!(!logs_contain("Renamed old bootstrap.json to .migrated"));
}
