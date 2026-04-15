//! Configuration for IronClaw.
//!
//! Settings are loaded with priority: env var > database > default.
//! `DATABASE_URL` lives in `~/.ironclaw/.env` (loaded via dotenvy early
//! in startup). Everything else comes from env vars, the DB settings
//! table, or auto-detection.

mod agent;
mod builder;
mod channels;
mod context;
mod database;
mod embeddings;
mod heartbeat;
pub(crate) mod helpers;
mod hygiene;
pub(crate) mod llm;
pub mod relay;
mod routines;
mod runtime_support;
mod safety;
mod sandbox;
mod secrets;
mod skills;
mod transcription;
mod tunnel;
mod wasm;

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use crate::error::ConfigError;
use crate::settings::Settings;

// Re-export all public types so `crate::config::FooConfig` continues to work.
pub use self::agent::AgentConfig;
pub use self::builder::BuilderModeConfig;
pub use self::channels::{ChannelsConfig, CliConfig, GatewayConfig, HttpConfig, SignalConfig};
pub use self::context::EnvContext;
pub use self::database::{DatabaseBackend, DatabaseConfig, SslMode, default_libsql_path};
pub use self::embeddings::EmbeddingsConfig;
pub use self::heartbeat::HeartbeatConfig;
pub use self::hygiene::HygieneConfig;
pub use self::llm::default_session_path;
pub use self::relay::RelayConfig;
pub use self::routines::RoutineConfig;
pub use self::runtime_support::{
    inject_llm_keys_from_secrets, inject_llm_keys_into_context, inject_os_credentials,
    inject_os_credentials_into_context, inject_single_var, remove_single_var,
};
pub use self::safety::SafetyConfig;
pub use self::sandbox::{ClaudeCodeConfig, SandboxModeConfig};
pub use self::secrets::SecretsConfig;
pub use self::skills::SkillsConfig;
pub use self::transcription::TranscriptionConfig;
pub use self::tunnel::TunnelConfig;
pub use self::wasm::WasmConfig;
pub use crate::llm::config::{
    BedrockConfig, CacheRetention, LlmConfig, NearAiConfig, OAUTH_PLACEHOLDER,
    RegistryProviderConfig,
};
pub use crate::llm::session::SessionConfig;

/// Thread-safe overlay for injected env vars (secrets loaded from DB).
///
/// Used by `inject_llm_keys_from_secrets()` to make API keys available to
/// `optional_env()` without unsafe `set_var` calls. `optional_env()` checks
/// real env vars first, then falls back to this overlay.
///
/// Uses `Mutex<HashMap>` instead of `OnceLock` so that both
/// `inject_os_credentials()` and `inject_llm_keys_from_secrets()` can merge
/// their data. Whichever runs first initialises the map; the second merges in.
static INJECTED_VARS: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Main configuration for the agent.
#[derive(Debug, Clone)]
pub struct Config {
    pub database: DatabaseConfig,
    pub llm: LlmConfig,
    pub embeddings: EmbeddingsConfig,
    pub tunnel: TunnelConfig,
    pub channels: ChannelsConfig,
    pub agent: AgentConfig,
    pub safety: SafetyConfig,
    pub wasm: WasmConfig,
    pub secrets: SecretsConfig,
    pub builder: BuilderModeConfig,
    pub heartbeat: HeartbeatConfig,
    pub hygiene: HygieneConfig,
    pub routines: RoutineConfig,
    pub sandbox: SandboxModeConfig,
    pub claude_code: ClaudeCodeConfig,
    pub skills: SkillsConfig,
    pub transcription: TranscriptionConfig,
    pub observability: crate::observability::ObservabilityConfig,
    /// Channel-relay integration (Slack via external relay service).
    /// Present only when both `CHANNEL_RELAY_URL` and `CHANNEL_RELAY_API_KEY` are set.
    pub relay: Option<RelayConfig>,
}

impl Config {
    /// Create a full Config for integration tests without reading env vars.
    ///
    /// Requires the `libsql` feature. Sets up:
    /// - libSQL database at the given path
    /// - WASM and embeddings disabled
    /// - Skills enabled with the given directories
    /// - Heartbeat, routines, sandbox, builder all disabled
    /// - Safety with injection check off, 100k output limit
    #[cfg(feature = "libsql")]
    pub async fn for_testing(
        libsql_path: std::path::PathBuf,
        skills_dir: std::path::PathBuf,
        installed_skills_dir: std::path::PathBuf,
    ) -> Result<Self, ConfigError> {
        let settings = Settings::default();
        let ctx =
            runtime_support::for_testing_context(&libsql_path, &skills_dir, &installed_skills_dir);

        let mut config = Self::from_context(&ctx, &settings).await?;
        config.llm = LlmConfig::for_testing();
        config.agent = AgentConfig::for_testing();
        config.embeddings = EmbeddingsConfig::default();
        config.tunnel = TunnelConfig::default();
        config.secrets = SecretsConfig::default();
        config.heartbeat = HeartbeatConfig::default();
        config.hygiene = HygieneConfig::default();
        config.claude_code = ClaudeCodeConfig::default();
        config.transcription = TranscriptionConfig::default();
        config.observability = crate::observability::ObservabilityConfig::default();
        config.relay = None;
        Ok(config)
    }

    /// Load configuration from environment variables and the database.
    ///
    /// Priority: env var > TOML config file > DB settings > default.
    /// This is the primary way to load config after DB is connected.
    pub async fn from_db(
        store: &(dyn crate::db::SettingsStore + Sync),
        user_id: &str,
    ) -> Result<Self, ConfigError> {
        Self::from_db_with_toml(store, user_id, None).await
    }

    /// Load from DB with an optional TOML config file overlay.
    pub async fn from_db_with_toml(
        store: &(dyn crate::db::SettingsStore + Sync),
        user_id: &str,
        toml_path: Option<&std::path::Path>,
    ) -> Result<Self, ConfigError> {
        let _ = dotenvy::dotenv();
        crate::bootstrap::load_ironclaw_env();

        // Load all settings from DB into a Settings struct
        let db_settings = match store
            .get_all_settings(crate::db::UserId::from(user_id))
            .await
        {
            Ok(map) => Settings::from_db_map(&map),
            Err(e) => {
                tracing::warn!("Failed to load settings from DB, using defaults: {}", e);
                Settings::default()
            }
        };

        let ctx = EnvContext::capture_ambient();
        let merged = runtime_support::merged_settings_with_toml(&db_settings, toml_path)?;
        Self::from_context(&ctx, &merged).await
    }

    /// Load configuration from environment variables only (no database).
    ///
    /// Used during early startup before the database is connected,
    /// and by CLI commands that don't have DB access.
    /// Falls back to legacy `settings.json` on disk if present.
    ///
    /// Loads both `./.env` (standard, higher priority) and `~/.ironclaw/.env`
    /// (lower priority) via dotenvy, which never overwrites existing vars.
    pub async fn from_env() -> Result<Self, ConfigError> {
        Self::from_env_with_toml(None).await
    }

    /// Load from env with an optional TOML config file overlay.
    pub async fn from_env_with_toml(
        toml_path: Option<&std::path::Path>,
    ) -> Result<Self, ConfigError> {
        let _ = dotenvy::dotenv();
        crate::bootstrap::load_ironclaw_env();
        let settings = Settings::load();
        let ctx = EnvContext::capture_ambient();
        let merged = runtime_support::merged_settings_with_toml(&settings, toml_path)?;
        Self::from_context(&ctx, &merged).await
    }

    /// Build config from an explicit environment snapshot and settings.
    ///
    /// Prefer this over `from_env*` and `from_db*` when the caller already has
    /// a stable snapshot of config inputs and wants deterministic resolution
    /// without ambient process reads during config construction.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> Result<(), ironclaw::error::ConfigError> {
    /// let ctx = ironclaw::config::EnvContext::default()
    ///     .with_env("DATABASE_BACKEND", "libsql")
    ///     .with_env("DATABASE_URL", "unused://test")
    ///     .with_env("LLM_BACKEND", "nearai");
    /// let settings = ironclaw::settings::Settings::default();
    /// let _config = ironclaw::config::Config::from_context(&ctx, &settings).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_context(ctx: &EnvContext, settings: &Settings) -> Result<Self, ConfigError> {
        Ok(Self {
            database: DatabaseConfig::resolve_from(ctx)?,
            llm: LlmConfig::resolve_from(ctx, settings)?,
            embeddings: EmbeddingsConfig::resolve_from(ctx, settings)?,
            tunnel: TunnelConfig::resolve_from(ctx, settings)?,
            channels: ChannelsConfig::resolve_from(ctx, settings)?,
            agent: AgentConfig::resolve_from(ctx, settings)?,
            safety: SafetyConfig::resolve_from(ctx)?,
            wasm: WasmConfig::resolve_from(ctx)?,
            secrets: SecretsConfig::resolve_from(ctx).await?,
            builder: BuilderModeConfig::resolve_from(ctx)?,
            heartbeat: HeartbeatConfig::resolve_from(ctx, settings)?,
            hygiene: HygieneConfig::resolve_from(ctx)?,
            routines: RoutineConfig::resolve_from(ctx)?,
            sandbox: SandboxModeConfig::resolve_from(ctx)?,
            claude_code: ClaudeCodeConfig::resolve_from(ctx)?,
            skills: SkillsConfig::resolve_from(ctx)?,
            transcription: TranscriptionConfig::resolve_from(ctx, settings)?,
            observability: crate::observability::ObservabilityConfig {
                backend: ctx
                    .get_owned("OBSERVABILITY_BACKEND")
                    .unwrap_or_else(|| "none".into()),
            },
            relay: RelayConfig::from_context(ctx),
        })
    }

    /// Build config from an explicit context plus a required TOML overlay.
    ///
    /// Use this when the caller already owns an [`EnvContext`] snapshot and
    /// wants the same deterministic resolution path as [`Self::from_context`],
    /// but with one additional TOML file merged into the supplied settings
    /// before config construction.
    ///
    /// Unlike the ambient `from_env_with_toml` and `from_db_with_toml`
    /// entrypoints, this method does not capture process environment or load
    /// bootstrap files on its own.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(path: &std::path::Path) -> Result<(), ironclaw::error::ConfigError> {
    /// let ctx = ironclaw::config::EnvContext::default()
    ///     .with_env("DATABASE_BACKEND", "libsql")
    ///     .with_env("DATABASE_URL", "unused://test");
    /// let settings = ironclaw::settings::Settings::default();
    /// let _config =
    ///     ironclaw::config::Config::from_context_with_toml(&ctx, &settings, path).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn from_context_with_toml(
        ctx: &EnvContext,
        settings: &Settings,
        toml_path: &std::path::Path,
    ) -> Result<Self, ConfigError> {
        let merged = runtime_support::merged_settings_with_toml(settings, Some(toml_path))?;
        Self::from_context(ctx, &merged).await
    }

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
        let settings = if let Some(store) = store {
            let mut s = match store
                .get_all_settings(crate::db::UserId::from(user_id))
                .await
            {
                Ok(map) => Settings::from_db_map(&map),
                Err(_) => Settings::default(),
            };
            runtime_support::apply_toml_overlay(&mut s, toml_path)?;
            s
        } else {
            Settings::default()
        };
        self.llm = LlmConfig::resolve(&settings)?;
        Ok(())
    }

    /// Re-resolve just the LLM portion of config from an explicit snapshot.
    ///
    /// This is the explicit-context companion to [`Self::re_resolve_llm`].
    /// Use it after mutating an [`EnvContext`] with credential overlays so the
    /// provider selection, base URL, and auth settings are recomputed without
    /// rebuilding unrelated config sections.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example() -> Result<(), ironclaw::error::ConfigError> {
    /// let settings = ironclaw::settings::Settings::default();
    /// let mut ctx = ironclaw::config::EnvContext::default()
    ///     .with_env("DATABASE_BACKEND", "libsql")
    ///     .with_env("DATABASE_URL", "unused://test")
    ///     .with_env("LLM_BACKEND", "anthropic");
    /// let mut config = ironclaw::config::Config::from_context(&ctx, &settings).await?;
    /// ctx.inject_secret("ANTHROPIC_API_KEY", "secret");
    /// config.re_resolve_llm_from(&ctx, &settings)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn re_resolve_llm_from(
        &mut self,
        ctx: &EnvContext,
        settings: &Settings,
    ) -> Result<(), ConfigError> {
        self.llm = LlmConfig::resolve_from(ctx, settings)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::{Config, DatabaseBackend, EnvContext};
    use crate::settings::Settings;

    fn base_context(base_dir: &Path) -> EnvContext {
        EnvContext::default()
            .with_env("IRONCLAW_BASE_DIR", base_dir.to_string_lossy())
            .with_env("DATABASE_BACKEND", "libsql")
            .with_env("DATABASE_URL", "unused://test")
    }

    #[tokio::test]
    async fn from_context_resolves_explicit_snapshot_inputs() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let base_dir = dir.path().join("ironclaw-home");
        let mut settings = Settings::default();
        settings.agent.name = "settings-agent".to_string();
        settings.heartbeat.enabled = true;
        settings
            .channels
            .wasm_channel_owner_ids
            .insert("signal".to_string(), 7);

        let ctx = base_context(&base_dir)
            .with_env("AGENT_NAME", "env-agent")
            .with_env("CLI_ENABLED", "false")
            .with_env("SAFETY_INJECTION_CHECK_ENABLED", "false")
            .with_env("LLM_BACKEND", "nearai")
            .with_env("NEARAI_MODEL", "env-model")
            .with_env("TELEGRAM_OWNER_ID", "99")
            .with_env("TRANSCRIPTION_ENABLED", "true")
            .with_env("OPENAI_API_KEY", "openai-test-key");

        let config = Config::from_context(&ctx, &settings)
            .await
            .expect("explicit context should resolve");

        assert_eq!(config.database.backend, DatabaseBackend::LibSql);
        assert_eq!(config.database.url(), "unused://test");
        assert_eq!(config.agent.name, "env-agent");
        assert!(config.heartbeat.enabled);
        assert!(!config.channels.cli.enabled);
        assert_eq!(config.channels.wasm_channels_dir, base_dir.join("channels"));
        assert_eq!(config.skills.local_dir, base_dir.join("skills"));
        assert_eq!(
            config.skills.installed_dir,
            base_dir.join("installed_skills")
        );
        assert_eq!(
            config.channels.wasm_channel_owner_ids.get("signal"),
            Some(&7)
        );
        assert_eq!(
            config.channels.wasm_channel_owner_ids.get("telegram"),
            Some(&99)
        );
        assert!(!config.safety.injection_check_enabled);
        assert_eq!(config.llm.nearai.model, "env-model");
        assert!(config.transcription.enabled);
        assert_eq!(
            config
                .transcription
                .openai_api_key
                .as_ref()
                .map(secrecy::ExposeSecret::expose_secret),
            Some("openai-test-key")
        );
    }

    #[tokio::test]
    async fn from_context_with_toml_merges_settings_before_resolution() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let base_dir = dir.path().join("ironclaw-home");
        let toml_path = dir.path().join("config.toml");
        fs::write(
            &toml_path,
            concat!(
                "[agent]\n",
                "name = \"toml-agent\"\n\n",
                "[heartbeat]\n",
                "enabled = true\n",
                "interval_secs = 900\n",
            ),
        )
        .expect("write TOML overlay");

        let mut settings = Settings::default();
        settings.agent.name = "settings-agent".to_string();

        let ctx = base_context(&base_dir).with_env("AGENT_NAME", "env-agent");
        let config = Config::from_context_with_toml(&ctx, &settings, &toml_path)
            .await
            .expect("context plus TOML should resolve");

        assert_eq!(config.agent.name, "env-agent");
        assert!(config.heartbeat.enabled);
        assert_eq!(config.heartbeat.interval_secs, 900);
    }

    #[tokio::test]
    async fn re_resolve_llm_from_rebuilds_llm_against_updated_snapshot() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let base_dir = dir.path().join("ironclaw-home");
        let settings = Settings::default();
        let ctx_a = base_context(&base_dir)
            .with_env("LLM_BACKEND", "nearai")
            .with_env("NEARAI_MODEL", "model-a");
        let ctx_b = base_context(&base_dir)
            .with_env("LLM_BACKEND", "nearai")
            .with_env("NEARAI_MODEL", "model-b");

        let mut config = Config::from_context(&ctx_a, &settings)
            .await
            .expect("initial context should resolve");
        assert_eq!(config.llm.nearai.model, "model-a");

        config
            .re_resolve_llm_from(&ctx_b, &settings)
            .expect("updated context should re-resolve");
        assert_eq!(config.llm.nearai.model, "model-b");
    }
}
