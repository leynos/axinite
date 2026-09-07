//! Unit tests for startup phase configuration and context loading.

use std::sync::Arc;

use axinite::cli::Cli;
use tokio::sync::OnceCell;

use super::*;

static LOADED_CONTEXT: OnceCell<LoadedConfigContextSnapshot> = OnceCell::const_new();

struct LoadedConfigContextSnapshot {
    config: Config,
    toml_path: Option<std::path::PathBuf>,
    session: Arc<axinite::llm::session::SessionManager>,
    log_broadcaster: Arc<LogBroadcaster>,
    log_level_handle: Arc<axinite::channels::web::log_layer::LogLevelHandle>,
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

fn phase_context() -> axinite::config::EnvContext {
    axinite::config::EnvContext::default()
        .with_env("DATABASE_BACKEND", "libsql")
        .with_env("LIBSQL_PATH", "/tmp/axinite-phase-smoke.db")
}

async fn loaded_context() -> anyhow::Result<LoadedConfigContext> {
    let snapshot = LOADED_CONTEXT
        .get_or_try_init(|| async {
            let loaded = phase_load_config_and_tracing_from(&cli_no_db(), &phase_context()).await?;
            anyhow::Ok(LoadedConfigContextSnapshot {
                config: loaded.config,
                toml_path: loaded.toml_path,
                session: loaded.session,
                log_broadcaster: loaded.log_broadcaster,
                log_level_handle: loaded.log_level_handle,
            })
        })
        .await?;

    Ok(LoadedConfigContext {
        config: snapshot.config.clone(),
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
