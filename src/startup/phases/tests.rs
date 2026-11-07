//! Unit tests for startup phase configuration and context loading.

use std::sync::Arc;

use axinite::{channels::web::log_layer::LogBroadcaster, cli::Cli};
use tokio::sync::OnceCell;

use super::*;

static LOADED_CONTEXT: OnceCell<LoadedConfigContextSnapshot> = OnceCell::const_new();

struct LoadedConfigContextSnapshot {
    config: Config,
    env_context: axinite::config::EnvContext,
    toml_path: Option<std::path::PathBuf>,
    session: Arc<axinite::llm::session::SessionManager>,
    log_broadcaster: Arc<LogBroadcaster>,
    log_level_handle: Arc<axinite::channels::web::log_layer::LogLevelHandle>,
    _database_dir: tempfile::TempDir,
}

fn cli_no_db() -> Cli {
    Cli {
        command: None,
        cli_only: false,
        no_db: true,
        message: None,
        config: None,
        no_onboard: false,
    }
}

fn phase_context(libsql_path: &std::path::Path) -> axinite::config::EnvContext {
    axinite::config::EnvContext::default()
        .with_env("DATABASE_BACKEND", "libsql")
        .with_env("LIBSQL_PATH", libsql_path.display().to_string())
}

/// Return a test config context built once from a temporary libSQL-backed
/// configuration and cached for later calls, cloning its resolved handles on
/// each call.
async fn loaded_context() -> anyhow::Result<LoadedConfigContext> {
    let snapshot = LOADED_CONTEXT
        .get_or_try_init(|| async {
            let database_dir = tempfile::tempdir()?;
            let database_path = database_dir.path().join("axinite-phase-smoke.db");
            let env = phase_context(&database_path);
            let loaded = phase_load_config_and_tracing_from(&cli_no_db(), &env).await?;
            anyhow::Ok(LoadedConfigContextSnapshot {
                config: loaded.config,
                env_context: loaded.env_context,
                toml_path: loaded.toml_path,
                session: loaded.session,
                log_broadcaster: loaded.log_broadcaster,
                log_level_handle: loaded.log_level_handle,
                _database_dir: database_dir,
            })
        })
        .await?;

    Ok(LoadedConfigContext {
        config: snapshot.config.clone(),
        env_context: snapshot.env_context.clone(),
        toml_path: snapshot.toml_path.clone(),
        session: Arc::clone(&snapshot.session),
        log_broadcaster: Arc::clone(&snapshot.log_broadcaster),
        log_level_handle: Arc::clone(&snapshot.log_level_handle),
        workspace_import_dir: None,
    })
}

#[tokio::test]
async fn load_config_and_tracing_smoke() {
    let loaded = loaded_context().await.expect("load ok");
    assert!(Arc::strong_count(&loaded.log_broadcaster) >= 1);
    assert!(Arc::strong_count(&loaded.session) >= 1);
}

#[tokio::test]
async fn build_components_smoke() {
    let cli = cli_no_db();
    let loaded = loaded_context().await.expect("load ok");
    let built = phase_build_components(&cli, loaded)
        .await
        .expect("build ok");
    let _ = built.components.tools;
}

/// Onboarding must hand startup a snapshot that already carries values entered
/// during the wizard, so startup does not fall back to the pre-onboarding one.
#[cfg(any(feature = "postgres", feature = "libsql"))]
#[tokio::test]
async fn onboarding_snapshot_preserves_the_wizard_snapshot() {
    use axinite::config::EnvContext;
    use axinite::setup::{SetupConfig, SetupWizard};

    const KEY_VAR: &str = "AXINITE_TEST_HANDOFF_SECRET";

    let dir = tempfile::tempdir().expect("create tempdir");
    let wizard = SetupWizard::with_config_and_env_context(
        SetupConfig::default(),
        EnvContext::default()
            .with_env("AXINITE_BASE_DIR", dir.path().display().to_string())
            .with_secret(KEY_VAR, "wizard-entered-value"),
    );

    let handoff = onboarding_snapshot(&wizard).expect("handoff should merge");

    assert_eq!(
        handoff.get(KEY_VAR),
        Some("wizard-entered-value"),
        "startup receives the wizard's snapshot, not the one captured before \
         onboarding"
    );
}

/// Scripted onboarding that returns a snapshot carrying the provider
/// credential the wizard would have collected interactively.
struct ScriptedOnboarding;

impl OnboardingRunner for ScriptedOnboarding {
    async fn run(&self, env_context: &EnvContext) -> anyhow::Result<EnvContext> {
        Ok(env_context
            .clone()
            .with_secret("NEARAI_API_KEY", "entered-during-onboarding"))
    }
}

fn onboarding_input(dir: &std::path::Path, db_path: &std::path::Path) -> EnvContext {
    EnvContext::default()
        .with_env("AXINITE_BASE_DIR", dir.display().to_string())
        .with_env("DATABASE_BACKEND", "libsql")
        .with_env("LIBSQL_PATH", db_path.display().to_string())
}

/// The startup phase must continue from the snapshot onboarding produced, and
/// the configuration loaded next must carry the credential entered during
/// onboarding. If startup used the snapshot captured before onboarding, the
/// credential would never reach configuration loading.
#[tokio::test]
async fn onboarding_credential_reaches_startup_configuration() -> anyhow::Result<()> {
    use secrecy::ExposeSecret;

    let dir = tempfile::tempdir()?;
    let db_dir = tempfile::tempdir()?;
    let input = onboarding_input(dir.path(), &db_dir.path().join("phase.db"));

    let (_pid_lock, returned) =
        phase_pid_and_onboard(&cli_no_db(), &input, &ScriptedOnboarding).await?;

    assert_eq!(
        returned.get("NEARAI_API_KEY"),
        Some("entered-during-onboarding"),
        "startup must continue from the post-onboarding snapshot"
    );

    let loaded = phase_load_config_and_tracing_from(&cli_no_db(), &returned).await?;
    assert_eq!(
        loaded
            .config
            .llm
            .nearai
            .api_key
            .as_ref()
            .map(|key| key.expose_secret()),
        Some("entered-during-onboarding"),
        "the onboarding credential must reach the startup configuration"
    );

    let stale = phase_load_config_and_tracing_from(&cli_no_db(), &input).await?;
    assert!(
        stale.config.llm.nearai.api_key.is_none(),
        "the pre-onboarding snapshot must not supply a provider credential, \
         so this assertion would fail if startup reused it"
    );
    Ok(())
}
