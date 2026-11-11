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

#[cfg(all(feature = "metrics", feature = "test-helpers"))]
mod telemetry {
    //! Counter assertions for the startup configuration-load production path.

    use super::*;
    use axinite::testing::metrics;

    /// Environment snapshot that resolves a complete configuration.
    fn resolvable_env(base_dir: &std::path::Path) -> EnvContext {
        EnvContext::default()
            .with_env("AXINITE_BASE_DIR", base_dir.display().to_string())
            .with_env("DATABASE_BACKEND", "libsql")
            .with_env("DATABASE_URL", "unused://test")
    }

    #[tokio::test]
    async fn successful_load_records_success_outcome() -> anyhow::Result<()> {
        let base_dir = tempfile::tempdir()?;
        let env = resolvable_env(base_dir.path());

        let _guard = metrics::record();
        load_initial_config_from(&env, None).await?;

        assert_eq!(
            metrics::counter("axinite.startup.config_load", "success"),
            1
        );
        Ok(())
    }

    #[tokio::test]
    async fn malformed_settings_records_settings_error_outcome() -> anyhow::Result<()> {
        let base_dir = tempfile::tempdir()?;
        ambient_fs::write(base_dir.path().join("settings.json"), "{ not json")?;
        let env = resolvable_env(base_dir.path());

        let _guard = metrics::record();
        let result = load_initial_config_from(&env, None).await;

        assert!(result.is_err(), "malformed settings must fail the load");
        assert_eq!(
            metrics::counter("axinite.startup.config_load", "settings_error"),
            1
        );
        assert_eq!(
            metrics::counter("axinite.startup.config_load", "success"),
            0
        );
        Ok(())
    }

    #[tokio::test]
    async fn invalid_backend_records_config_error_outcome() -> anyhow::Result<()> {
        let base_dir = tempfile::tempdir()?;
        let env = EnvContext::default()
            .with_env("AXINITE_BASE_DIR", base_dir.path().display().to_string())
            .with_env("DATABASE_BACKEND", "not-a-backend");

        let _guard = metrics::record();
        let result = load_initial_config_from(&env, None).await;

        assert!(result.is_err(), "an invalid backend must fail the load");
        assert_eq!(
            metrics::counter("axinite.startup.config_load", "config_error"),
            1
        );
        assert_eq!(
            metrics::counter("axinite.startup.config_load", "success"),
            0
        );
        Ok(())
    }
}
