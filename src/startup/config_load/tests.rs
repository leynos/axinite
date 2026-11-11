//! Unit tests for startup configuration loading from a snapshot.

use super::*;
use std::collections::HashMap;

#[cfg(feature = "libsql")]
#[tokio::test]
/// Verify config loading uses the explicit environment snapshot supplied by
/// the onboarding flow.
async fn config_load_uses_the_onboarding_environment_snapshot() -> anyhow::Result<()> {
    let base_dir = tempfile::tempdir()?;
    let database_path = base_dir.path().join("onboarded.db");
    let env_context = EnvContext::for_testing(
        HashMap::from([
            (
                "AXINITE_BASE_DIR".to_string(),
                base_dir.path().display().to_string(),
            ),
            ("DATABASE_BACKEND".to_string(), "libsql".to_string()),
            (
                "LIBSQL_PATH".to_string(),
                database_path.display().to_string(),
            ),
        ]),
        HashMap::new(),
    );
    let config = load_initial_config_from(&env_context, None).await?;

    assert_eq!(
        config.database.backend,
        axinite::config::DatabaseBackend::LibSql
    );
    assert_eq!(
        config.database.libsql_path.as_deref(),
        Some(database_path.as_path())
    );
    Ok(())
}

#[test]
fn settings_are_loaded_from_the_context_base_directory() -> anyhow::Result<()> {
    let base_dir = tempfile::tempdir()?;
    let settings_path = base_dir.path().join("settings.json");
    ambient_fs::write(&settings_path, r#"{"llm_backend":"ollama"}"#)?;
    let env =
        EnvContext::default().with_env("AXINITE_BASE_DIR", base_dir.path().display().to_string());

    let settings = load_settings_from_context(&env)?;

    assert_eq!(settings.llm_backend.as_deref(), Some("ollama"));
    Ok(())
}
