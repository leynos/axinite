//! Tests for bootstrap migration and upsert helpers.

use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use rstest::{fixture, rstest};
use tempfile::{TempDir, tempdir};
use tracing_test::traced_test;

use crate::testing::test_utils::EnvVarsGuard;

use super::super::*;

#[derive(Clone, Copy)]
enum RenameSetup {
    ExistingFile,
    MissingFile,
    #[cfg(unix)]
    ReadOnlyDirectory,
}

struct RenameFixture {
    dir: TempDir,
    path: std::path::PathBuf,
    #[cfg(unix)]
    original_dir_permissions: Option<std::fs::Permissions>,
}

impl RenameFixture {
    fn prepare(&mut self, setup: RenameSetup) {
        match setup {
            RenameSetup::ExistingFile => self.write_legacy_file(),
            RenameSetup::MissingFile => {}
            #[cfg(unix)]
            RenameSetup::ReadOnlyDirectory => {
                self.write_legacy_file();
                self.make_dir_read_only();
            }
        }
    }

    fn write_legacy_file(&self) {
        std::fs::write(&self.path, "{}").expect("write legacy settings file");
    }

    #[cfg(unix)]
    fn make_dir_read_only(&mut self) {
        self.original_dir_permissions = Some(
            std::fs::metadata(self.dir.path())
                .expect("read directory metadata")
                .permissions(),
        );
        std::fs::set_permissions(self.dir.path(), std::fs::Permissions::from_mode(0o555))
            .expect("make directory read-only");
    }

    #[cfg(not(unix))]
    fn make_dir_read_only(&mut self) {}

    fn migrated_path(&self) -> std::path::PathBuf {
        self.dir.path().join("settings.json.migrated")
    }
}

#[cfg(unix)]
impl Drop for RenameFixture {
    fn drop(&mut self) {
        if let Some(permissions) = self.original_dir_permissions.take() {
            std::fs::set_permissions(self.dir.path(), permissions)
                .expect("restore temp directory permissions");
        }
    }
}

#[fixture]
fn rename_fixture() -> RenameFixture {
    let dir = tempdir().expect("create temp dir for rename test");
    let path = dir.path().join("settings.json");
    RenameFixture {
        dir,
        path,
        #[cfg(unix)]
        original_dir_permissions: None,
    }
}

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

#[traced_test]
#[rstest]
#[case::success(RenameSetup::ExistingFile, None)]
#[case::missing_source(RenameSetup::MissingFile, Some(ErrorKind::NotFound))]
#[cfg_attr(
    unix,
    case::permission_denied(RenameSetup::ReadOnlyDirectory, Some(ErrorKind::PermissionDenied))
)]
fn rename_to_migrated_cases(
    mut rename_fixture: RenameFixture,
    #[case] setup: RenameSetup,
    #[case] expected_error_kind: Option<ErrorKind>,
) {
    rename_fixture.prepare(setup);

    let result = super::super::migration::rename_to_migrated(&rename_fixture.path);

    match expected_error_kind {
        Some(kind) => {
            let error = result.expect_err("rename should fail for this case");
            assert_eq!(error.kind(), kind);
            assert!(logs_contain("Failed to rename"));
        }
        None => {
            result.expect("rename legacy settings");
            assert!(!rename_fixture.path.exists());
            assert!(rename_fixture.migrated_path().exists());
            assert!(!logs_contain("Failed to rename"));
        }
    }
}

#[traced_test]
#[rstest]
fn rename_legacy_bootstrap_success(mut rename_fixture: RenameFixture) {
    rename_fixture.path = rename_fixture.dir.path().join("bootstrap.json");
    rename_fixture.prepare(RenameSetup::ExistingFile);

    super::super::migration::rename_legacy_bootstrap(rename_fixture.dir.path());

    assert!(
        rename_fixture
            .dir
            .path()
            .join("bootstrap.json.migrated")
            .exists()
    );
    assert!(!logs_contain("Failed to rename"));
    assert!(logs_contain("Renamed old bootstrap.json to .migrated"));
}

#[cfg(unix)]
#[traced_test]
#[rstest]
fn rename_legacy_bootstrap_permission_denied(mut rename_fixture: RenameFixture) {
    rename_fixture.path = rename_fixture.dir.path().join("bootstrap.json");
    rename_fixture.prepare(RenameSetup::ReadOnlyDirectory);

    super::super::migration::rename_legacy_bootstrap(rename_fixture.dir.path());

    assert!(logs_contain("Failed to rename"));
    assert!(!logs_contain("Renamed old bootstrap.json to .migrated"));
}
