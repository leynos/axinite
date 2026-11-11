//! Re-resolve LLM settings after credentials change.

use crate::config::{Config, ConfigError, EnvContext, runtime_support};
use crate::llm::config::LlmConfig;
use crate::settings::Settings;

/// Inputs used to re-resolve an LLM configuration after credential injection.
pub struct LlmResolutionContext<'a> {
    /// Environment snapshot containing the effective configuration values.
    pub env_context: &'a EnvContext,
    /// Optional settings store used to resolve provider settings.
    pub settings_store: Option<&'a (dyn crate::db::SettingsStore + Sync)>,
    /// User whose stored settings should be loaded.
    pub user_id: &'a str,
    /// Optional TOML overlay applied to stored settings.
    pub toml_path: Option<&'a std::path::Path>,
}

impl Config {
    /// Re-resolve only the LLM config after credential injection.
    ///
    /// Called by `AppBuilder::init_secrets()` after injecting API keys into
    /// the env overlay. Only rebuilds `self.llm` — all other config fields
    /// are unaffected, preserving values from the initial config load (or
    /// from `Config::for_testing()` in test mode).
    pub async fn re_resolve_llm(
        &mut self,
        store: Option<&(dyn crate::db::SettingsStore + Sync)>,
        user_id: &str,
        toml_path: Option<&std::path::Path>,
    ) -> Result<(), ConfigError> {
        EnvContext::refresh_dotenv_layers();
        let env_context = EnvContext::capture_ambient();
        self.re_resolve_llm_with_context(LlmResolutionContext {
            env_context: &env_context,
            settings_store: store,
            user_id,
            toml_path,
        })
        .await
    }

    /// Re-resolve only the LLM config from an explicit environment snapshot.
    pub async fn re_resolve_llm_with_context(
        &mut self,
        context: LlmResolutionContext<'_>,
    ) -> Result<(), ConfigError> {
        let LlmResolutionContext {
            env_context,
            settings_store,
            user_id,
            toml_path,
        } = context;
        let settings = if let Some(store) = settings_store {
            let mut settings = match store
                .get_all_settings(crate::db::UserId::from(user_id))
                .await
            {
                Ok(map) => Settings::from_db_map(&map),
                Err(_) => Settings::default(),
            };
            runtime_support::apply_toml_overlay(&mut settings, toml_path)?;
            settings
        } else {
            Settings::default()
        };
        self.llm = LlmConfig::resolve_from(env_context, &settings)?;
        Ok(())
    }
}
