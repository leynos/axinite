//! Unit tests for startup configuration loading from a snapshot.

use super::*;

#[cfg(feature = "libsql")]
#[tokio::test]
/// Verify config loading merges bootstrap values written during onboarding,
/// so the libSQL backend and database path from `.env` are reflected in the
/// loaded config.
async fn config_load_includes_bootstrap_values_written_during_onboarding() -> anyhow::Result<()> {
    let base_dir = tempfile::tempdir()?;
    let database_path = base_dir.path().join("onboarded.db");
    ambient_fs::write(
        base_dir.path().join(".env"),
        format!(
            "DATABASE_BACKEND=libsql\nLIBSQL_PATH={}\n",
            database_path.display()
        ),
    )?;
    let initial_context =
        EnvContext::default().with_env("AXINITE_BASE_DIR", base_dir.path().display().to_string());

    let onboarding_context = axinite::bootstrap::merge_axinite_env_into_context(&initial_context)?;
    let config = load_initial_config_from(&onboarding_context, None).await?;

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
