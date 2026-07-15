//! Main setup wizard orchestration.
//!
//! The wizard guides users through:
//! 1. Database connection
//! 2. Security (secrets master key)
//! 3. Inference provider (NEAR AI, Anthropic, OpenAI, Ollama, OpenAI-compatible)
//! 4. Model selection
//! 5. Embeddings
//! 6. Channel configuration
//! 7. Extensions (tool installation from registry)
//! 8. Docker sandbox
//! 9. Heartbeat (background tasks)

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[cfg(feature = "postgres")]
use deadpool_postgres::Config as PoolConfig;
use secrecy::{ExposeSecret, SecretString};

use crate::bootstrap::ironclaw_base_dir;
use crate::channels::wasm::{
    ChannelCapabilitiesFile, available_channel_names, install_bundled_channel,
};
use crate::config::OAUTH_PLACEHOLDER;
use crate::llm::{SessionConfig, SessionManager};
use crate::secrets::{SecretsCrypto, SecretsStore};
use crate::settings::{KeySource, Settings};
use crate::setup::channels::{
    SecretsContext, setup_http, setup_signal, setup_tunnel, setup_wasm_channel,
};
use crate::setup::persistence::DefaultSettingsPersistence;
use crate::setup::prompts::{
    confirm, input, optional_input, print_error, print_header, print_info, print_step,
    print_success, secret_input, select_many, select_one,
};

// unused const, keep commented for clarity / future use
// const CHANNEL_INDEX_CLI: usize = 0;
const CHANNEL_INDEX_HTTP: usize = 1;
const CHANNEL_INDEX_SIGNAL: usize = 2;

/// Setup wizard error.
#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Channel setup error: {0}")]
    Channel(String),

    #[error("User cancelled")]
    Cancelled,
}

impl From<crate::setup::channels::ChannelSetupError> for SetupError {
    fn from(e: crate::setup::channels::ChannelSetupError) -> Self {
        SetupError::Channel(e.to_string())
    }
}

/// Setup wizard configuration.
#[derive(Debug, Clone, Default)]
pub struct SetupConfig {
    /// Skip authentication step (use existing session).
    pub skip_auth: bool,
    /// Only reconfigure channels.
    pub channels_only: bool,
    /// Only reconfigure LLM provider and model selection.
    pub provider_only: bool,
    /// Quick setup: auto-defaults everything except LLM provider and model.
    pub quick: bool,
}

/// Interactive setup wizard for IronClaw.
pub struct SetupWizard {
    config: SetupConfig,
    settings: Settings,
    session_manager: Option<Arc<SessionManager>>,
    /// Database pool (created during setup, postgres only).
    #[cfg(feature = "postgres")]
    db_pool: Option<deadpool_postgres::Pool>,
    /// libSQL backend (created during setup, libsql only).
    #[cfg(feature = "libsql")]
    db_backend: Option<Arc<crate::db::libsql::LibSqlBackend>>,
    /// Secrets crypto (created during setup).
    secrets_crypto: Option<Arc<SecretsCrypto>>,
    /// Cached API key from provider setup (used by model fetcher without env mutation).
    llm_api_key: Option<SecretString>,
}

mod channel_catalog;
mod channels;
mod database;
mod database_ops;
#[cfg(feature = "libsql")]
mod database_prompts;
mod extensions;
mod lifecycle;
mod model_catalog;
mod models;
mod persist;
mod provider_flows;
mod provider_vendors;
mod providers;
mod sandbox;
mod security;
mod summary;

#[cfg(test)]
mod tests;

/// Whether a DATABASE_BACKEND value selects the libSQL backend.
#[cfg(feature = "libsql")]
fn is_libsql_backend(backend: &str) -> bool {
    matches!(backend, "libsql" | "turso" | "sqlite")
}

/// Whether a DATABASE_BACKEND value selects the PostgreSQL backend.
#[cfg(feature = "postgres")]
fn is_postgres_backend(backend: &str) -> bool {
    matches!(backend, "postgres" | "postgresql")
}

/// Whether a tool's auth summary declares an authentication method.
fn requires_auth(auth: &crate::registry::manifest::AuthSummary) -> bool {
    auth.method.as_deref() != Some("none") && auth.method.is_some()
}

/// The provider-specific base URL variable and value, when the provider
/// defines one distinct from the generic LLM_BASE_URL/OLLAMA_BASE_URL vars.
fn provider_base_url_var(
    registry: &crate::llm::ProviderRegistry,
    backend: &str,
) -> Option<(String, String)> {
    let def = registry.find(backend)?;
    let base_url_env = def.base_url_env.as_ref()?;
    let base_url = def.default_base_url.as_ref()?;
    let is_generic = base_url_env == "LLM_BASE_URL" || base_url_env == "OLLAMA_BASE_URL";
    if is_generic {
        return None;
    }
    Some((base_url_env.clone(), base_url.clone()))
}

/// Mask password in a database URL for display.
#[cfg(feature = "postgres")]
fn mask_password_in_url(url: &str) -> String {
    // URL format: scheme://user:password@host/database
    // Find "://" to locate start of credentials
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let credentials_start = scheme_end + 3; // After "://"

    // Find "@" to locate end of credentials
    let Some(at_pos) = url[credentials_start..].find('@') else {
        return url.to_string();
    };
    let at_abs = credentials_start + at_pos;

    // Find ":" in the credentials section (separates user from password)
    let credentials = &url[credentials_start..at_abs];
    let Some(colon_pos) = credentials.find(':') else {
        return url.to_string();
    };

    // Build masked URL: scheme://user:****@host/database
    let scheme = &url[..credentials_start]; // "postgres://"
    let username = &credentials[..colon_pos]; // "user"
    let after_at = &url[at_abs..]; // "@localhost/db"

    format!("{}{}:****{}", scheme, username, after_at)
}

/// Mask an API key for display: show first 6 + last 4 chars.
///
/// Uses char-based indexing to avoid panicking on multi-byte UTF-8.
fn mask_api_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() < 12 {
        let prefix: String = chars.iter().take(4).collect();
        return format!("{prefix}...");
    }
    let prefix: String = chars[..6].iter().collect();
    let suffix: String = chars[chars.len() - 4..].iter().collect();
    format!("{prefix}...{suffix}")
}

/// Capitalize the first letter of a string.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}
