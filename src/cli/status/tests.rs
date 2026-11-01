//! Unit tests for status command settings loading.

use super::{db_backend_name, load_settings_from};
use crate::config::EnvContext;

#[test]
fn db_backend_name_uses_the_resolved_libsql_backend() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("axinite.db");
    ambient_fs::write(&db_path, "").expect("write libSQL marker");
    let context =
        crate::bootstrap::with_libsql_backend_autodetection(EnvContext::default(), &db_path);

    assert_eq!(db_backend_name(&context), "libsql");
}

/// Regression test for #354: load_settings_from must read config.toml.
#[test]
fn reads_toml_heartbeat_enabled() {
    let dir = tempfile::tempdir().expect("tempdir");
    let json_path = dir.path().join("settings.json");
    let toml_path = dir.path().join("config.toml");

    // No JSON file — only TOML
    ambient_fs::write(
        &toml_path,
        "[heartbeat]\nenabled = true\ninterval_secs = 600",
    )
    .expect("write toml");

    let settings = load_settings_from(&json_path, &toml_path);
    assert!(settings.heartbeat.enabled);
    assert_eq!(settings.heartbeat.interval_secs, 600);
}

/// Without any config files, defaults are returned.
#[test]
fn defaults_without_config_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let settings = load_settings_from(
        &dir.path().join("nonexistent.json"),
        &dir.path().join("nonexistent.toml"),
    );
    assert!(!settings.heartbeat.enabled);
}

/// settings.json is respected.
#[test]
fn reads_json_heartbeat_enabled() {
    let dir = tempfile::tempdir().expect("tempdir");
    let json_path = dir.path().join("settings.json");
    let toml_path = dir.path().join("nonexistent.toml");

    ambient_fs::write(
        &json_path,
        r#"{"heartbeat":{"enabled":true,"interval_secs":900}}"#,
    )
    .expect("write json");

    let settings = load_settings_from(&json_path, &toml_path);
    assert!(settings.heartbeat.enabled);
    assert_eq!(settings.heartbeat.interval_secs, 900);
}

/// TOML overlay wins over JSON settings.
#[test]
fn toml_overlay_wins_over_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let json_path = dir.path().join("settings.json");
    let toml_path = dir.path().join("config.toml");

    ambient_fs::write(
        &json_path,
        r#"{"heartbeat":{"enabled":false,"interval_secs":100}}"#,
    )
    .expect("write json");
    ambient_fs::write(
        &toml_path,
        "[heartbeat]\nenabled = true\ninterval_secs = 200",
    )
    .expect("write toml");

    let settings = load_settings_from(&json_path, &toml_path);
    assert!(settings.heartbeat.enabled);
    assert_eq!(settings.heartbeat.interval_secs, 200);
}

/// Invalid TOML is warned but doesn't crash; falls back to JSON/defaults.
#[test]
fn invalid_toml_falls_back_gracefully() {
    let dir = tempfile::tempdir().expect("tempdir");
    let json_path = dir.path().join("settings.json");
    let toml_path = dir.path().join("config.toml");

    ambient_fs::write(
        &json_path,
        r#"{"heartbeat":{"enabled":true,"interval_secs":500}}"#,
    )
    .expect("write json");
    ambient_fs::write(&toml_path, "this is not valid toml [[[").expect("write bad toml");

    let settings = load_settings_from(&json_path, &toml_path);
    // Should fall back to JSON values, not crash
    assert!(settings.heartbeat.enabled);
    assert_eq!(settings.heartbeat.interval_secs, 500);
}
