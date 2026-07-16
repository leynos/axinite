//! Unit tests for startup phase configuration and context loading.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use ironclaw::cli::Cli;
use tokio::sync::OnceCell;

use super::*;

static PHASES_ENV_MUTEX: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
static LOADED_CONTEXT: OnceCell<LoadedConfigContextSnapshot> = OnceCell::const_new();

struct LoadedConfigContextSnapshot {
    config: Config,
    toml_path: Option<std::path::PathBuf>,
    session: Arc<ironclaw::llm::session::SessionManager>,
    log_broadcaster: Arc<LogBroadcaster>,
    log_level_handle: Arc<ironclaw::channels::web::log_layer::LogLevelHandle>,
}

struct EnvVarsGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    originals: HashMap<&'static str, Option<String>>,
}

impl EnvVarsGuard {
    fn new(keys: &[&'static str]) -> Self {
        // Recover from poisoning: the guard only snapshots and restores
        // environment variables, so the protected state stays coherent.
        let lock = PHASES_ENV_MUTEX
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let originals = keys
            .iter()
            .map(|key| (*key, std::env::var(key).ok()))
            .collect();
        Self {
            _lock: lock,
            originals,
        }
    }

    fn set(&mut self, key: &'static str, value: &str) {
        self.originals
            .entry(key)
            .or_insert_with(|| std::env::var(key).ok());
        // SAFETY: EnvVarsGuard holds PHASES_ENV_MUTEX for its entire lifetime.
        unsafe { std::env::set_var(key, value) };
    }
}

impl Drop for EnvVarsGuard {
    fn drop(&mut self) {
        for (key, value) in &self.originals {
            // SAFETY: EnvVarsGuard holds PHASES_ENV_MUTEX for its entire lifetime.
            unsafe {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }
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

fn phase_env_guard() -> EnvVarsGuard {
    let mut guard = EnvVarsGuard::new(&["DATABASE_BACKEND", "DATABASE_URL", "LIBSQL_PATH"]);
    guard.set("DATABASE_BACKEND", "libsql");
    guard.set("LIBSQL_PATH", "/tmp/ironclaw-phase-smoke.db");
    guard
}

async fn loaded_context() -> anyhow::Result<LoadedConfigContext> {
    let snapshot = LOADED_CONTEXT
        .get_or_try_init(|| async {
            let _env_guard = phase_env_guard();
            let loaded = phase_load_config_and_tracing(&cli_no_db()).await?;
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
