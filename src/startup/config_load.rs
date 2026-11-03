//! Configuration and tracing loading for the host binary startup.

use std::sync::Arc;

use axinite::{
    channels::web::log_layer::LogBroadcaster,
    cli::Cli,
    config::{Config, EnvContext},
    llm::create_session_manager,
};

use crate::startup::context::LoadedConfigContext;

/// Load configuration and tracing from an explicit startup environment snapshot.
pub(crate) async fn phase_load_config_and_tracing_from(
    cli: &Cli,
    env: &EnvContext,
) -> anyhow::Result<LoadedConfigContext> {
    let toml_path = cli.config.clone();
    let config = load_initial_config_from(env, toml_path.as_deref()).await?;

    let session = create_session_manager(config.llm.session.clone()).await;
    let log_broadcaster = Arc::new(LogBroadcaster::new());
    let log_level_handle =
        axinite::channels::web::log_layer::init_tracing(Arc::clone(&log_broadcaster));

    tracing::debug!("Starting Axinite...");
    tracing::debug!("Loaded configuration for agent: {}", config.agent.name);
    tracing::debug!("LLM backend: {}", config.llm.backend);

    Ok(LoadedConfigContext {
        config,
        env_context: env.clone(),
        toml_path,
        session,
        log_broadcaster,
        log_level_handle,
        workspace_import_dir: env
            .get("WORKSPACE_IMPORT_DIR")
            .map(std::path::PathBuf::from),
    })
}

/// Build the initial configuration from `env` and the optional TOML overlay.
///
/// The caller supplies the startup environment snapshot; this function never
/// re-reads process variables. A missing `settings.json` yields defaults. An
/// unreadable or malformed one is surfaced through
/// [`axinite::error::ConfigError`] rather than silently replaced by defaults.
async fn load_initial_config_from(
    env: &EnvContext,
    toml_path: Option<&std::path::Path>,
) -> anyhow::Result<Config> {
    let settings = match load_settings_from_context(env) {
        Ok(settings) => settings,
        Err(error) => {
            super::telemetry::config_load("settings_error");
            return Err(error.into());
        }
    };
    // `None` still applies the default `~/.axinite/config.toml` overlay, so
    // omitting `--config` does not silently drop settings that only exist
    // there. The reload path applies the same default overlay.
    let result = Config::from_context_with_optional_toml(env, &settings, toml_path).await;
    match result {
        Ok(config) => {
            super::telemetry::config_load("success");
            Ok(config)
        }
        Err(axinite::error::ConfigError::MissingRequired { key, hint }) => {
            super::telemetry::config_load("config_error");
            anyhow::bail!(
                "Configuration error: Missing required setting '{}'. {}. \
                 Run 'axinite onboard' to configure, or set the required environment variables.",
                key,
                hint
            );
        }
        Err(error) => {
            super::telemetry::config_load("config_error");
            Err(error.into())
        }
    }
}

/// Load persisted settings from `env`'s base directory.
///
/// The base directory comes from the snapshot, so a caller can point settings
/// resolution at a test directory without touching the process environment.
fn load_settings_from_context(
    env: &EnvContext,
) -> Result<axinite::settings::Settings, axinite::error::ConfigError> {
    let settings_path = env.axinite_base_dir().join("settings.json");
    Ok(axinite::settings::Settings::load_from(&settings_path)?)
}

#[cfg(test)]
mod tests;
