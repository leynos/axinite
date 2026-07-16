//! Unit tests for application wiring and start-up configuration.

use super::*;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use crate::{
    channels::web::log_layer::LogBroadcaster,
    config::Config,
    db::{Database, SettingKey, UserId},
    llm::{LlmProvider, SessionConfig, SessionManager},
    testing::StubLlm,
    workspace::Workspace,
};
use anyhow::Context;
use rstest::{fixture, rstest};

#[cfg(feature = "libsql")]
use crate::db::libsql::LibSqlBackend;

#[test]
fn runtime_side_effects_new_all_none_does_not_panic() {
    let _ = RuntimeSideEffects::new(None, None, None, false);
}

#[tokio::test]
async fn runtime_side_effects_start_no_ops_when_nothing_configured() -> anyhow::Result<()> {
    let se = RuntimeSideEffects::new(None, None, None, false);
    se.start()?.wait_until_bootstrapped().await?;
    Ok(())
}

async fn assert_no_activation(workspace: &Arc<Workspace>, import_dir: &Path) -> anyhow::Result<()> {
    assert!(
        tokio::fs::try_exists(import_dir.join("MARKER.md")).await?,
        "build_components() must not mutate the source import directory"
    );
    assert!(
        !workspace.exists("MARKER.md").await?,
        "build_components() must not run deferred workspace import"
    );
    assert!(
        !workspace.exists(crate::workspace::paths::README).await?,
        "build_components() must not run seed_if_empty()"
    );
    Ok(())
}

#[cfg(feature = "libsql")]
#[fixture]
async fn two_phase_fixture() -> anyhow::Result<(AppBuilder, PathBuf, tempfile::TempDir)> {
    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path().join("app-builder-test.db");
    let backend = LibSqlBackend::new_local(&db_path).await?;
    backend.run_migrations().await?;
    let db: Arc<dyn Database> = Arc::new(backend);

    let skills_dir = temp_dir.path().join("skills");
    let installed_skills_dir = temp_dir.path().join("installed_skills");
    let workspace_import_dir = temp_dir.path().join("workspace_import");
    tokio::fs::create_dir_all(&skills_dir).await?;
    tokio::fs::create_dir_all(&installed_skills_dir).await?;
    tokio::fs::create_dir_all(&workspace_import_dir).await?;
    tokio::fs::write(
        workspace_import_dir.join("MARKER.md"),
        "# Marker\n\nImported by RuntimeSideEffects::start().\n",
    )
    .await?;

    let config = Config::for_testing(db_path, skills_dir, installed_skills_dir).await?;
    let session = Arc::new(SessionManager::new(SessionConfig::default()));
    let log_broadcaster = Arc::new(LogBroadcaster::new());
    let llm: Arc<dyn LlmProvider> = Arc::new(StubLlm::new("ok"));

    let mut builder = AppBuilder::new(AppBuilderParams {
        config,
        flags: AppBuilderFlags {
            workspace_import_dir: Some(workspace_import_dir.clone()),
            ..AppBuilderFlags::default()
        },
        toml_path: None,
        session,
        log_broadcaster,
    });
    builder.with_database(db);
    builder.with_llm(llm);

    Ok((builder, workspace_import_dir, temp_dir))
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn init_database_migrates_legacy_disk_settings() -> anyhow::Result<()> {
    if std::env::var("IRONCLAW_APP_MIGRATION_CHILD")
        .ok()
        .as_deref()
        == Some("1")
    {
        run_init_database_migration_child().await?;
        return Ok(());
    }

    let temp_dir = tempfile::tempdir()?;
    let ironclaw_dir = temp_dir.path().join("ironclaw");
    let db_path = temp_dir.path().join("app-migration.db");
    let skills_dir = temp_dir.path().join("skills");
    let installed_skills_dir = temp_dir.path().join("installed_skills");
    ambient_fs::create_dir_all(&ironclaw_dir)?;
    ambient_fs::create_dir_all(&skills_dir)?;
    ambient_fs::create_dir_all(&installed_skills_dir)?;
    ambient_fs::write(
        ironclaw_dir.join("settings.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "onboard_completed": true,
            "database_backend": "libsql"
        }))?,
    )?;

    let current_exe = std::env::current_exe()?;
    let status = Command::new(current_exe)
        .args([
            "--exact",
            "app::tests::init_database_migrates_legacy_disk_settings",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("IRONCLAW_APP_MIGRATION_CHILD", "1")
        .env("IRONCLAW_BASE_DIR", &ironclaw_dir)
        .env("IRONCLAW_APP_MIGRATION_DB_PATH", &db_path)
        .env("IRONCLAW_APP_MIGRATION_SKILLS_DIR", &skills_dir)
        .env(
            "IRONCLAW_APP_MIGRATION_INSTALLED_SKILLS_DIR",
            &installed_skills_dir,
        )
        .env_remove("DATABASE_URL")
        .env("DATABASE_BACKEND", "libsql")
        .env("LIBSQL_PATH", &db_path)
        .status()?;

    assert!(status.success(), "child boundary test failed: {status}");
    Ok(())
}

#[cfg(feature = "libsql")]
async fn run_init_database_migration_child() -> anyhow::Result<()> {
    let ironclaw_dir = PathBuf::from(std::env::var("IRONCLAW_BASE_DIR")?);
    let db_path = PathBuf::from(std::env::var("IRONCLAW_APP_MIGRATION_DB_PATH")?);
    let skills_dir = PathBuf::from(std::env::var("IRONCLAW_APP_MIGRATION_SKILLS_DIR")?);
    let installed_skills_dir = PathBuf::from(std::env::var(
        "IRONCLAW_APP_MIGRATION_INSTALLED_SKILLS_DIR",
    )?);

    let config = Config::for_testing(db_path, skills_dir, installed_skills_dir).await?;
    let session = Arc::new(SessionManager::new(SessionConfig::default()));
    let log_broadcaster = Arc::new(LogBroadcaster::new());
    let mut builder = AppBuilder::new(AppBuilderParams {
        config,
        flags: AppBuilderFlags::default(),
        toml_path: None,
        session,
        log_broadcaster,
    });

    builder.init_database().await?;

    let db = builder
        .db
        .as_ref()
        .context("init_database should store db")?;
    let migrated = db
        .get_setting(
            UserId::from("default"),
            SettingKey::from("onboard_completed"),
        )
        .await?;
    assert_eq!(migrated, Some(serde_json::Value::Bool(true)));
    assert!(!ironclaw_dir.join("settings.json").exists());
    assert!(ironclaw_dir.join("settings.json.migrated").exists());
    Ok(())
}

#[cfg(feature = "libsql")]
#[rstest]
#[tokio::test]
async fn build_components_returns_without_activating_side_effects(
    #[future] two_phase_fixture: anyhow::Result<(AppBuilder, PathBuf, tempfile::TempDir)>,
) -> anyhow::Result<()> {
    let (builder, workspace_import_dir, _temp_dir) = two_phase_fixture.await?;
    let (components, side_effects) = builder.build_components().await?;
    assert!(components.tools.count() > 0);
    let workspace = components
        .workspace
        .as_ref()
        .context("workspace should be constructed during build_components()")?;
    assert_no_activation(workspace, &workspace_import_dir).await?;
    side_effects.start()?.wait_until_bootstrapped().await?;
    let marker = workspace.read("MARKER.md").await?;
    assert_eq!(
        marker.content,
        "# Marker\n\nImported by RuntimeSideEffects::start().\n"
    );

    Ok(())
}

#[cfg(feature = "libsql")]
#[rstest]
#[tokio::test]
async fn build_all_waits_for_workspace_bootstrap(
    #[future] two_phase_fixture: anyhow::Result<(AppBuilder, PathBuf, tempfile::TempDir)>,
) -> anyhow::Result<()> {
    let (builder, _workspace_import_dir, _temp_dir) = two_phase_fixture.await?;
    let components = builder.build_all().await?;
    let workspace = components
        .workspace
        .as_ref()
        .context("workspace should be constructed during build_all()")?;
    assert!(
        workspace.exists(crate::workspace::paths::README).await?,
        "build_all() must complete workspace seeding before returning"
    );
    let marker = workspace.read("MARKER.md").await?;
    assert_eq!(
        marker.content,
        "# Marker\n\nImported by RuntimeSideEffects::start().\n"
    );
    Ok(())
}
