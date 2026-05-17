//! Tests for legacy migration rename helpers.

use std::io::ErrorKind;

use rstest::rstest;
use tracing_test::traced_test;

use super::migration_support::{RenameSetup, rename_fixture};

#[traced_test]
#[rstest]
#[case::success(RenameSetup::ExistingFile, None)]
#[case::missing_source(RenameSetup::MissingFile, Some(ErrorKind::NotFound))]
#[cfg_attr(
    unix,
    case::permission_denied(RenameSetup::ReadOnlyDirectory, Some(ErrorKind::PermissionDenied))
)]
fn rename_to_migrated_cases(
    #[case] setup: RenameSetup,
    #[case] expected_error_kind: Option<ErrorKind>,
) {
    let mut fixture = rename_fixture();
    fixture.prepare(setup);

    let result = super::super::migration::rename_to_migrated(&fixture.path);

    match expected_error_kind {
        Some(kind) => {
            let error = result.expect_err("rename should fail for this case");
            assert_eq!(error.kind(), kind);
            assert!(logs_contain("Failed to rename"));
        }
        None => {
            result.expect("rename legacy settings");
            assert!(!fixture.path.exists());
            assert!(fixture.migrated_path().exists());
        }
    }
}

#[traced_test]
#[rstest]
fn rename_legacy_bootstrap_success() {
    let mut fixture = rename_fixture();
    fixture.path = fixture.dir.path().join("bootstrap.json");
    fixture.prepare(RenameSetup::ExistingFile);

    super::super::migration::rename_legacy_bootstrap(fixture.dir.path());

    assert!(fixture.migrated_path().exists());
    assert!(logs_contain("Renamed old bootstrap.json to .migrated"));
}

#[cfg(unix)]
#[traced_test]
#[rstest]
fn rename_legacy_bootstrap_permission_denied() {
    let mut fixture = rename_fixture();
    fixture.path = fixture.dir.path().join("bootstrap.json");
    fixture.prepare(RenameSetup::ReadOnlyDirectory);

    super::super::migration::rename_legacy_bootstrap(fixture.dir.path());

    assert!(logs_contain("Failed to rename"));
    assert!(!logs_contain("Renamed old bootstrap.json to .migrated"));
}
