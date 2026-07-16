//! Tests for TOML persistence, merging, and default path resolution.

use crate::settings::*;

#[test]
fn toml_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let mut settings = Settings::default();
    settings.agent.name = "toml-bot".to_string();
    settings.heartbeat.enabled = true;
    settings.heartbeat.interval_secs = 900;

    settings.save_toml(&path).unwrap();
    let loaded = Settings::load_toml(&path).unwrap().unwrap();

    assert_eq!(loaded.agent.name, "toml-bot");
    assert!(loaded.heartbeat.enabled);
    assert_eq!(loaded.heartbeat.interval_secs, 900);
}

/// Regression test: /model command must persist selected_model to TOML config.
/// Prior to the fix, `set_model()` only changed the in-memory provider and the
/// choice was lost on restart.
#[test]
fn toml_selected_model_update_persists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    // Start with a config that has a different model.
    let settings = Settings {
        selected_model: Some("old-model".to_string()),
        ..Default::default()
    };
    settings.save_toml(&path).unwrap();

    // Simulate what persist_selected_model does: load, update, save.
    let mut loaded = Settings::load_toml(&path).unwrap().unwrap();
    loaded.selected_model = Some("new-model".to_string());
    loaded.save_toml(&path).unwrap();

    // Verify the change survived a reload.
    let reloaded = Settings::load_toml(&path).unwrap().unwrap();
    assert_eq!(reloaded.selected_model, Some("new-model".to_string()));
}

#[test]
fn toml_missing_file_returns_none() {
    let result = Settings::load_toml(std::path::Path::new("/tmp/nonexistent_config.toml"));
    assert!(result.unwrap().is_none());
}

#[test]
fn toml_invalid_content_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    ambient_fs::write(&path, "this is not valid toml [[[").unwrap();

    let result = Settings::load_toml(&path);
    assert!(result.is_err());
}

#[test]
fn toml_partial_config_uses_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("partial.toml");

    // Only set agent name, everything else should be default
    ambient_fs::write(&path, "[agent]\nname = \"partial-bot\"\n").unwrap();

    let loaded = Settings::load_toml(&path).unwrap().unwrap();
    assert_eq!(loaded.agent.name, "partial-bot");
    // Defaults preserved
    assert_eq!(loaded.agent.max_parallel_jobs, 5);
    assert!(!loaded.heartbeat.enabled);
}

#[test]
fn toml_header_comment_present() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    Settings::default().save_toml(&path).unwrap();
    let content = ambient_fs::read_to_string(&path).unwrap();

    assert!(content.starts_with("# IronClaw configuration file."));
    assert!(content.contains("[agent]"));
    assert!(content.contains("[heartbeat]"));
}

#[test]
fn merge_only_overrides_non_default_values() {
    let mut base = Settings::default();
    base.agent.name = "from-db".to_string();
    base.heartbeat.interval_secs = 600;

    let mut toml_overlay = Settings::default();
    toml_overlay.agent.name = "from-toml".to_string();

    base.merge_from(&toml_overlay);

    assert_eq!(base.agent.name, "from-toml");
    assert_eq!(base.heartbeat.interval_secs, 600);
}

#[test]
fn merge_preserves_base_when_overlay_is_default() {
    let mut base = Settings::default();
    base.agent.name = "custom-name".to_string();
    base.heartbeat.enabled = true;

    let overlay = Settings::default();
    base.merge_from(&overlay);

    assert_eq!(base.agent.name, "custom-name");
    assert!(base.heartbeat.enabled);
}

#[test]
fn toml_creates_parent_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("deep").join("config.toml");

    Settings::default().save_toml(&path).unwrap();
    assert!(path.exists());
}

#[test]
fn default_toml_path_under_ironclaw() {
    let path = Settings::default_toml_path();
    assert!(path.to_string_lossy().contains(".ironclaw"));
    assert!(path.to_string_lossy().ends_with("config.toml"));
}
