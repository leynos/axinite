//! Shared runtime helpers for configuration assembly.
//!
//! These helpers keep `config::mod` focused on composing resolved sections
//! while centralising TOML overlays, test contexts, and credential injection.

use std::collections::HashMap;

use super::{ClaudeCodeConfig, EnvContext, INJECTED_VARS};
use crate::error::ConfigError;
use crate::settings::Settings;

pub(crate) fn merged_settings_with_toml(
    base: &Settings,
    toml_path: Option<&std::path::Path>,
) -> Result<Settings, ConfigError> {
    let mut merged = base.clone();
    apply_toml_overlay(&mut merged, toml_path)?;
    Ok(merged)
}

pub(crate) async fn inject_llm_keys_with<HasValue, SetValue>(
    secrets: &dyn crate::secrets::SecretsStore,
    user_id: &str,
    mut has_value: HasValue,
    mut set_value: SetValue,
) where
    HasValue: FnMut(&str) -> bool,
    SetValue: FnMut(&str, String),
{
    for (secret_name, env_var) in secret_mappings() {
        if has_value(&env_var) {
            continue;
        }
        if let Ok(decrypted) = secrets.get_decrypted(user_id, &secret_name).await {
            let value = decrypted.expose().to_string();
            if value.trim().is_empty() {
                continue;
            }
            set_value(&env_var, value);
            tracing::debug!("Loaded secret '{}' for env var '{}'", secret_name, env_var);
        }
    }
}

#[cfg(feature = "libsql")]
pub(crate) fn for_testing_context(
    libsql_path: &std::path::Path,
    skills_dir: &std::path::Path,
    installed_skills_dir: &std::path::Path,
) -> EnvContext {
    let test_channels_dir = skills_dir
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("ironclaw-test-channels");

    EnvContext::default()
        .with_env("DATABASE_BACKEND", "libsql")
        .with_env("DATABASE_URL", "unused://test")
        .with_env("DATABASE_POOL_SIZE", "1")
        .with_env("DATABASE_SSLMODE", "disable")
        .with_env("LIBSQL_PATH", libsql_path.to_string_lossy())
        .with_env("WASM_CHANNELS_DIR", test_channels_dir.to_string_lossy())
        .with_env("CLI_ENABLED", "false")
        .with_env("WASM_CHANNELS_ENABLED", "false")
        .with_env("SAFETY_INJECTION_CHECK_ENABLED", "false")
        .with_env("WASM_ENABLED", "false")
        .with_env("BUILDER_ENABLED", "false")
        .with_env("ROUTINES_ENABLED", "false")
        .with_env("SANDBOX_ENABLED", "false")
        .with_env("SKILLS_ENABLED", "true")
        .with_env("SKILLS_DIR", skills_dir.to_string_lossy())
        .with_env(
            "SKILLS_INSTALLED_DIR",
            installed_skills_dir.to_string_lossy(),
        )
}

/// Load and merge a TOML config file into settings.
///
/// If `explicit_path` is `Some`, loads from that path (errors are fatal).
/// If `None`, tries the default path `~/.ironclaw/config.toml` (missing
/// file is silently ignored).
pub(crate) fn apply_toml_overlay(
    settings: &mut Settings,
    explicit_path: Option<&std::path::Path>,
) -> Result<(), ConfigError> {
    let path = explicit_path
        .map(std::path::PathBuf::from)
        .unwrap_or_else(Settings::default_toml_path);
    // An explicit path must exist; the default path may be absent.
    apply_toml_overlay_at(settings, &path, explicit_path.is_none())
}

fn apply_toml_overlay_at(
    settings: &mut Settings,
    path: &std::path::Path,
    optional_when_missing: bool,
) -> Result<(), ConfigError> {
    match Settings::load_toml(path) {
        Ok(Some(toml_settings)) => {
            settings.merge_from(&toml_settings);
            tracing::debug!("Loaded TOML config from {}", path.display());
        }
        Ok(None) => {
            if !optional_when_missing {
                return Err(ConfigError::ParseError(format!(
                    "Config file not found: {}",
                    path.display()
                )));
            }
        }
        Err(e) => {
            if !optional_when_missing {
                return Err(ConfigError::ParseError(format!(
                    "Failed to load config file {}: {}",
                    path.display(),
                    e
                )));
            }
            tracing::warn!("Failed to load default config file: {}", e);
        }
    }
    Ok(())
}

/// Load API keys from the encrypted secrets store into a thread-safe overlay.
///
/// This bridges the gap between secrets stored during onboarding and the
/// env-var-first resolution in `LlmConfig::resolve()`. Keys in the overlay
/// are read by `optional_env()` before falling back to `std::env::var()`,
/// so explicit env vars always win.
///
/// Also loads tokens from OS credential stores (macOS Keychain, Linux
/// credentials files) which don't require the secrets DB.
pub async fn inject_llm_keys_from_secrets(
    secrets: &dyn crate::secrets::SecretsStore,
    user_id: &str,
) {
    let mut injected = HashMap::new();
    inject_llm_keys_with(
        secrets,
        user_id,
        |env_var| matches!(std::env::var(env_var), Ok(val) if !val.is_empty()),
        |env_var, value| {
            injected.insert(env_var.to_string(), value);
        },
    )
    .await;

    inject_os_credential_store_tokens(&mut injected);
    merge_injected_vars(injected);
}

/// Inject decrypted LLM credentials into an explicit [`EnvContext`].
///
/// This mirrors [`inject_llm_keys_from_secrets`] without mutating the process
/// environment or the global injected overlay. Existing values already present
/// in `ctx` still win over secrets-store values.
///
/// # Examples
///
/// ```no_run
/// # async fn example(
/// #     secrets: &dyn crate::secrets::SecretsStore,
/// # ) {
/// let mut ctx = crate::config::EnvContext::default();
/// crate::config::inject_llm_keys_into_context(&mut ctx, secrets, "user-123").await;
/// # }
/// ```
pub async fn inject_llm_keys_into_context(
    ctx: &mut EnvContext,
    secrets: &dyn crate::secrets::SecretsStore,
    user_id: &str,
) {
    let mut injected = HashMap::new();
    inject_llm_keys_with(
        secrets,
        user_id,
        |env_var| ctx.get(env_var).is_some(),
        |env_var, value| {
            injected.insert(env_var.to_string(), value);
        },
    )
    .await;
    ctx.merge_secrets(injected);
    inject_os_credentials_into_context(ctx);
}

/// Load tokens from OS credential stores (no DB required).
///
/// Called unconditionally during startup, even when the encrypted secrets DB
/// is unavailable. This ensures OAuth tokens from `claude login` are available
/// for config resolution.
pub fn inject_os_credentials() {
    let mut injected = HashMap::new();
    inject_os_credential_store_tokens(&mut injected);
    merge_injected_vars(injected);
}

/// Inject OAuth tokens from OS credential stores into an explicit context.
///
/// This is the explicit-context equivalent of [`inject_os_credentials`]. It is
/// typically used during startup after capturing ambient env vars but before
/// calling [`crate::config::Config::from_context`] or
/// [`crate::config::Config::re_resolve_llm_from`].
pub fn inject_os_credentials_into_context(ctx: &mut EnvContext) {
    let mut injected = HashMap::new();
    inject_os_credential_store_tokens(&mut injected);
    ctx.merge_secrets(injected);
}

/// Inject a single key-value pair into the overlay.
///
/// Used by the setup wizard to make credentials available to `optional_env()`
/// without calling `unsafe { std::env::set_var }`.
pub fn inject_single_var(key: &str, value: &str) {
    match INJECTED_VARS.lock() {
        Ok(mut map) => {
            map.insert(key.to_string(), value.to_string());
        }
        Err(poisoned) => {
            poisoned
                .into_inner()
                .insert(key.to_string(), value.to_string());
        }
    }
}

/// Remove a single key from the overlay.
pub fn remove_single_var(key: &str) {
    match INJECTED_VARS.lock() {
        Ok(mut map) => {
            map.remove(key);
        }
        Err(poisoned) => {
            poisoned.into_inner().remove(key);
        }
    }
}

fn secret_mappings() -> Vec<(String, String)> {
    let mut mappings: Vec<(String, String)> = vec![
        (
            "llm_nearai_api_key".to_string(),
            "NEARAI_API_KEY".to_string(),
        ),
        (
            "llm_anthropic_oauth_token".to_string(),
            "ANTHROPIC_OAUTH_TOKEN".to_string(),
        ),
    ];

    let registry = crate::llm::ProviderRegistry::load();
    mappings.extend(registry.selectable().iter().filter_map(|def| {
        def.api_key_env.as_ref().and_then(|env_var| {
            def.setup
                .as_ref()
                .and_then(|s| s.secret_name())
                .map(|secret_name| (secret_name.to_string(), env_var.clone()))
        })
    }));
    mappings
}

/// Merge new entries into the global injected-vars overlay.
///
/// New keys are inserted; existing keys are overwritten (later callers win,
/// e.g. fresh OS credential store tokens override stale DB copies).
fn merge_injected_vars(new_entries: HashMap<String, String>) {
    if new_entries.is_empty() {
        return;
    }
    match INJECTED_VARS.lock() {
        Ok(mut map) => map.extend(new_entries),
        Err(poisoned) => poisoned.into_inner().extend(new_entries),
    }
}

/// Shared helper: extract tokens from OS credential stores into the overlay map.
fn inject_os_credential_store_tokens(injected: &mut HashMap<String, String>) {
    // Try the OS credential store for a fresh Anthropic OAuth token.
    // Tokens from `claude login` expire in 8-12h, so the DB copy may be stale.
    // A fresh extraction from macOS Keychain / Linux credentials.json wins
    // over the (possibly expired) copy stored in the encrypted secrets DB.
    if let Some(fresh) = ClaudeCodeConfig::extract_oauth_token() {
        injected.insert("ANTHROPIC_OAUTH_TOKEN".to_string(), fresh);
        tracing::debug!("Refreshed ANTHROPIC_OAUTH_TOKEN from OS credential store");
    }
}
