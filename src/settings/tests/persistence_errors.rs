//! Regression tests for error reporting when loading `settings.json`.
//!
//! A *missing* file is still a normal first-run condition, but a file that
//! exists and cannot be read or parsed must surface as an error instead of
//! silently replacing the user's configuration with defaults.

use crate::config::{Config, EnvContext};
use crate::error::ConfigError;
use crate::settings::{Settings, SettingsError};

#[test]
fn missing_settings_file_loads_defaults() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    let path = dir.path().join("settings.json");

    let settings = Settings::load_from(&path).expect("a missing settings file is not an error");

    assert_eq!(settings.agent.name, Settings::default().agent.name);
}

#[test]
/// Verify malformed settings JSON surfaces a parse error naming the file
/// rather than silently falling back to defaults.
fn malformed_settings_file_reports_parse_error() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    let path = dir.path().join("settings.json");
    ambient_fs::write(&path, "{ not json").expect("write malformed settings");

    let error = Settings::load_from(&path).expect_err("malformed JSON must be reported");

    assert!(
        matches!(error, SettingsError::Parse { .. }),
        "expected a parse error, got {error:?}"
    );
    assert!(
        error.to_string().contains("settings.json"),
        "error should name the file: {error}"
    );
}

#[test]
/// Verify a settings path that exists but cannot be read as a file surfaces
/// a read error.
fn unreadable_settings_file_reports_read_error() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    let path = dir.path().join("settings.json");
    // A directory exists at the settings path but cannot be read as a file,
    // which exercises the non-`NotFound` I/O branch.
    ambient_fs::create_dir(&path).expect("create directory at settings path");

    let error = Settings::load_from(&path).expect_err("an unreadable path must be reported");

    assert!(
        matches!(error, SettingsError::Read { .. }),
        "expected a read error, got {error:?}"
    );
}

#[tokio::test]
/// Verify config loading propagates a malformed settings file as a
/// configuration parse error instead of swallowing it.
async fn config_loading_propagates_settings_errors() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    ambient_fs::write(dir.path().join("settings.json"), "{ not json")
        .expect("write malformed settings");
    let ctx = EnvContext::default().with_env("AXINITE_BASE_DIR", dir.path().display().to_string());

    let error = Config::from_env_from_context(&ctx)
        .await
        .expect_err("config loading must not swallow a settings error");

    assert!(
        matches!(error, ConfigError::ParseError(_)),
        "expected a configuration parse error, got {error:?}"
    );
}
