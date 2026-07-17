//! Unit tests for configuration resolution from the environment.

use std::path::Path;

use ambient_fs as fs;

use super::{Config, DatabaseBackend, EnvContext};
use crate::settings::Settings;

fn base_context(base_dir: &Path) -> EnvContext {
    EnvContext::default()
        .with_env("AXINITE_BASE_DIR", base_dir.to_string_lossy())
        .with_env("DATABASE_BACKEND", "libsql")
        .with_env("DATABASE_URL", "unused://test")
}

#[tokio::test]
async fn from_context_resolves_explicit_snapshot_inputs() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let base_dir = dir.path().join("axinite-home");
    let mut settings = Settings::default();
    settings.agent.name = "settings-agent".to_string();
    settings.heartbeat.enabled = true;
    settings
        .channels
        .wasm_channel_owner_ids
        .insert("signal".to_string(), 7);

    let ctx = base_context(&base_dir)
        .with_env("AGENT_NAME", "env-agent")
        .with_env("CLI_ENABLED", "false")
        .with_env("SAFETY_INJECTION_CHECK_ENABLED", "false")
        .with_env("LLM_BACKEND", "nearai")
        .with_env("NEARAI_MODEL", "env-model")
        .with_env("TELEGRAM_OWNER_ID", "99")
        .with_env("TRANSCRIPTION_ENABLED", "true")
        .with_env("OPENAI_API_KEY", "openai-test-key");

    let config = Config::from_context(&ctx, &settings)
        .await
        .expect("explicit context should resolve");

    assert_eq!(config.database.backend, DatabaseBackend::LibSql);
    assert_eq!(config.database.url(), "unused://test");
    assert_eq!(config.agent.name, "env-agent");
    assert!(config.heartbeat.enabled);
    assert!(!config.channels.cli.enabled);
    assert_eq!(config.channels.wasm_channels_dir, base_dir.join("channels"));
    assert_eq!(config.skills.local_dir, base_dir.join("skills"));
    assert_eq!(
        config.skills.installed_dir,
        base_dir.join("installed_skills")
    );
    assert_eq!(
        config.channels.wasm_channel_owner_ids.get("signal"),
        Some(&7)
    );
    assert_eq!(
        config.channels.wasm_channel_owner_ids.get("telegram"),
        Some(&99)
    );
    assert!(!config.safety.injection_check_enabled);
    assert_eq!(config.llm.nearai.model, "env-model");
    assert!(config.transcription.enabled);
    assert_eq!(
        config
            .transcription
            .openai_api_key
            .as_ref()
            .map(secrecy::ExposeSecret::expose_secret),
        Some("openai-test-key")
    );
}

#[tokio::test]
async fn from_context_with_toml_merges_settings_before_resolution() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let base_dir = dir.path().join("axinite-home");
    let toml_path = dir.path().join("config.toml");
    fs::write(
        &toml_path,
        concat!(
            "[agent]\n",
            "name = \"toml-agent\"\n\n",
            "[heartbeat]\n",
            "enabled = true\n",
            "interval_secs = 900\n",
        ),
    )
    .expect("write TOML overlay");

    let mut settings = Settings::default();
    settings.agent.name = "settings-agent".to_string();

    let ctx = base_context(&base_dir).with_env("AGENT_NAME", "env-agent");
    let config = Config::from_context_with_toml(&ctx, &settings, &toml_path)
        .await
        .expect("context plus TOML should resolve");

    assert_eq!(config.agent.name, "env-agent");
    assert!(config.heartbeat.enabled);
    assert_eq!(config.heartbeat.interval_secs, 900);
}

#[tokio::test]
async fn re_resolve_llm_from_rebuilds_llm_against_updated_snapshot() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let base_dir = dir.path().join("axinite-home");
    let settings = Settings::default();
    let ctx_a = base_context(&base_dir)
        .with_env("LLM_BACKEND", "nearai")
        .with_env("NEARAI_MODEL", "model-a");
    let ctx_b = base_context(&base_dir)
        .with_env("LLM_BACKEND", "nearai")
        .with_env("NEARAI_MODEL", "model-b");

    let mut config = Config::from_context(&ctx_a, &settings)
        .await
        .expect("initial context should resolve");
    assert_eq!(config.llm.nearai.model, "model-a");

    config
        .re_resolve_llm_from(&ctx_b, &settings)
        .expect("updated context should re-resolve");
    assert_eq!(config.llm.nearai.model, "model-b");
}
