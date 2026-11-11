//! Unit tests for application wiring and start-up configuration.

use super::*;
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use crate::{
    channels::web::log_layer::LogBroadcaster,
    config::{Config, EnvContext},
    db::{Database, SettingKey, UserId},
    llm::{LlmProvider, SessionConfig, SessionManager},
    testing::StubLlm,
    workspace::Workspace,
};
use anyhow::Context;
use rstest::{fixture, rstest};

#[cfg(feature = "libsql")]
use crate::db::libsql::LibSqlBackend;

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
/// Build an `AppBuilder` over a migrated temporary libSQL database, with a
/// workspace import directory containing a marker file.
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

    let mut builder = AppBuilder::new_with_context(
        AppBuilderParams {
            config,
            flags: AppBuilderFlags {
                workspace_import_dir: Some(workspace_import_dir.clone()),
                ..AppBuilderFlags::default()
            },
            toml_path: None,
            session,
            log_broadcaster,
        },
        EnvContext::default().with_secret("ANTHROPIC_API_KEY", "overlay-only-key"),
    );
    builder.with_database(db);
    builder.with_llm(llm);

    Ok((builder, workspace_import_dir, temp_dir))
}

#[cfg(feature = "libsql")]
#[tokio::test]
/// Verify that `init_database` migrates legacy `settings.json` into the
/// database and renames the file. The test re-executes itself as a child
/// process so the migration runs with a controlled environment.
async fn init_database_migrates_legacy_disk_settings() -> anyhow::Result<()> {
    if {
        #[expect(clippy::disallowed_methods, reason = "FIXME: migrate to EnvContext, see https://github.com/leynos/axinite/issues/333")]
        std::env::var("AXINITE_APP_MIGRATION_CHILD")
    }
    .ok()
    .as_deref()
        == Some("1")
    {
        run_init_database_migration_child().await?;
        return Ok(());
    }

    let temp_dir = tempfile::tempdir()?;
    let axinite_dir = temp_dir.path().join("axinite");
    let db_path = temp_dir.path().join("app-migration.db");
    let skills_dir = temp_dir.path().join("skills");
    let installed_skills_dir = temp_dir.path().join("installed_skills");
    ambient_fs::create_dir_all(&axinite_dir)?;
    ambient_fs::create_dir_all(&skills_dir)?;
    ambient_fs::create_dir_all(&installed_skills_dir)?;
    ambient_fs::write(
        axinite_dir.join("settings.json"),
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
        .env("AXINITE_APP_MIGRATION_CHILD", "1")
        .env("AXINITE_BASE_DIR", &axinite_dir)
        .env("AXINITE_APP_MIGRATION_DB_PATH", &db_path)
        .env("AXINITE_APP_MIGRATION_SKILLS_DIR", &skills_dir)
        .env(
            "AXINITE_APP_MIGRATION_INSTALLED_SKILLS_DIR",
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
/// Child-process half of the legacy settings migration test: read the paths
/// from the environment, run `init_database`, and assert the setting was
/// migrated and the legacy file renamed.
async fn run_init_database_migration_child() -> anyhow::Result<()> {
    let axinite_dir = PathBuf::from({
        #[expect(
            clippy::disallowed_methods,
            reason = "FIXME: migrate to EnvContext, see https://github.com/leynos/axinite/issues/333"
        )]
        std::env::var("AXINITE_BASE_DIR")
    }?);
    let db_path = PathBuf::from({
        #[expect(
            clippy::disallowed_methods,
            reason = "FIXME: migrate to EnvContext, see https://github.com/leynos/axinite/issues/333"
        )]
        std::env::var("AXINITE_APP_MIGRATION_DB_PATH")
    }?);
    let skills_dir = PathBuf::from({
        #[expect(
            clippy::disallowed_methods,
            reason = "FIXME: migrate to EnvContext, see https://github.com/leynos/axinite/issues/333"
        )]
        std::env::var("AXINITE_APP_MIGRATION_SKILLS_DIR")
    }?);
    let installed_skills_dir = PathBuf::from({
        #[expect(
            clippy::disallowed_methods,
            reason = "FIXME: migrate to EnvContext, see https://github.com/leynos/axinite/issues/333"
        )]
        std::env::var("AXINITE_APP_MIGRATION_INSTALLED_SKILLS_DIR")
    }?);

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
    assert!(!axinite_dir.join("settings.json").exists());
    assert!(axinite_dir.join("settings.json.migrated").exists());
    Ok(())
}

#[cfg(feature = "libsql")]
#[rstest]
#[tokio::test]
/// Verify that `build_components` constructs the workspace without importing
/// or seeding it, and that `RuntimeSideEffects::start` performs the deferred
/// import.
async fn build_components_returns_without_activating_side_effects(
    #[future] two_phase_fixture: anyhow::Result<(AppBuilder, PathBuf, tempfile::TempDir)>,
) -> anyhow::Result<()> {
    let (builder, workspace_import_dir, _temp_dir) = two_phase_fixture.await?;
    let (components, side_effects, env_context) = builder.build_components_with_context().await?;
    assert_eq!(
        env_context.get("ANTHROPIC_API_KEY"),
        Some("overlay-only-key")
    );
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

/// `init_secrets` must resolve stored LLM credentials from the snapshot alone.
///
/// The builder is given backend handles and a master key directly, so the
/// secrets store is created without any database phase or ambient state. A
/// stored Anthropic key then has to reach both the builder's environment
/// snapshot and its re-resolved LLM config; a key already present in the
/// snapshot must win over the stored value. `build_components_with_context`
/// returns this same snapshot, whose survival is asserted separately by
/// `build_components_returns_without_activating_side_effects`.
#[cfg(feature = "libsql")]
#[tokio::test]
async fn init_secrets_injects_store_credentials_into_the_snapshot() -> anyhow::Result<()> {
    use secrecy::{ExposeSecret, SecretString};

    use crate::secrets::{
        CreateSecretParams, LibSqlSecretsStore, NativeSecretsStore, SecretsCrypto,
    };

    const MASTER_KEY: &str = "builder-test-master-key-0123456789abcdef";
    const STORED_KEY: &str = "store-anthropic-key";

    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path().join("builder-secrets.db");
    let skills_dir = temp_dir.path().join("skills");
    let installed_skills_dir = temp_dir.path().join("installed_skills");
    let cases: [(&str, Option<&str>, &str); 2] = [
        ("store only", None, STORED_KEY),
        (
            "snapshot wins",
            Some("snapshot-anthropic-key"),
            "snapshot-anthropic-key",
        ),
    ];

    for (label, snapshot_key, expected) in cases {
        let mut config = Config::for_testing(
            db_path.clone(),
            skills_dir.clone(),
            installed_skills_dir.clone(),
        )
        .await?;
        config.secrets.master_key = Some(SecretString::from(MASTER_KEY));

        // Take the handles from the production connection path rather than
        // assembling them by hand, so the builder sees the same backend shape
        // that startup provides.
        let (_, handles) = crate::db::connect_with_handles(&config.database).await?;
        let shared_db = handles
            .libsql_db
            .clone()
            .context("libsql connection must expose a shared database")?;
        let crypto = Arc::new(SecretsCrypto::new(SecretString::from(MASTER_KEY))?);
        let store = LibSqlSecretsStore::new(shared_db, crypto);
        NativeSecretsStore::create(
            &store,
            "default",
            CreateSecretParams::new("llm_anthropic_api_key", STORED_KEY),
        )
        .await?;

        let mut env = EnvContext::default().with_env("LLM_BACKEND", "anthropic");
        if let Some(key) = snapshot_key {
            env = env.with_env("ANTHROPIC_API_KEY", key);
        }

        let mut builder = AppBuilder::new_with_context(
            AppBuilderParams {
                config,
                flags: AppBuilderFlags::default(),
                toml_path: None,
                session: Arc::new(SessionManager::new(SessionConfig::default())),
                log_broadcaster: Arc::new(LogBroadcaster::new()),
            },
            env,
        );
        builder.handles = Some(handles);

        builder.init_secrets().await?;

        assert!(
            builder.secrets_store.is_some(),
            "case {label}: a master key and backend handle must create a store"
        );
        assert_eq!(
            builder.env_context.get("ANTHROPIC_API_KEY"),
            Some(expected),
            "case {label}: snapshot credential"
        );
        let api_key = builder
            .config
            .llm
            .provider
            .as_ref()
            .and_then(|provider| provider.api_key.as_ref())
            .map(ExposeSecret::expose_secret);
        assert_eq!(
            api_key,
            Some(expected),
            "case {label}: LLM config must re-resolve from the snapshot"
        );
    }

    Ok(())
}
