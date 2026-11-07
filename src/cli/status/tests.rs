//! Unit tests for status command settings loading.

use super::{db_backend_name, load_settings_from, settings_for_context, status_report};
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

/// Status must load settings from the snapshot's base directory, so a caller
/// supplying a divergent `AXINITE_BASE_DIR` observes that deployment.
#[test]
fn settings_for_context_reads_snapshot_selected_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    ambient_fs::write(
        dir.path().join("settings.json"),
        r#"{"heartbeat":{"enabled":true,"interval_secs":654}}"#,
    )
    .expect("write snapshot settings.json");
    let context = EnvContext::default().with_env(
        "AXINITE_BASE_DIR",
        dir.path().to_str().expect("UTF-8 temp path"),
    );

    let settings = settings_for_context(&context);

    assert!(settings.heartbeat.enabled);
    assert_eq!(settings.heartbeat.interval_secs, 654);
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

/// The status command boundary must render the deployment selected by the
/// supplied snapshot, not the ambient one, so its paths and settings change
/// with `AXINITE_BASE_DIR`.
#[tokio::test]
async fn status_report_uses_the_snapshot_base_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    ambient_fs::write(
        dir.path().join("settings.json"),
        r#"{"heartbeat":{"enabled":true,"interval_secs":432}}"#,
    )
    .expect("write snapshot settings.json");
    let context = EnvContext::default().with_env(
        "AXINITE_BASE_DIR",
        dir.path().to_str().expect("UTF-8 temp path"),
    );

    let report = status_report(&context).await;

    assert!(
        report.contains(&format!(
            "Config:      {}",
            dir.path().join(".env").display()
        )),
        "report must name the snapshot's config path, got:\n{report}"
    );
    assert!(
        report.contains("Heartbeat:   enabled (interval: 432s)"),
        "report must render settings from the snapshot base directory, got:\n{report}"
    );
}
