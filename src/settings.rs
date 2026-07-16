//! User settings persistence.
//!
//! Stores user preferences in ~/.ironclaw/settings.json.
//! Settings are loaded with env var > settings.json > default priority.

mod access;
mod connectivity;
mod execution;
mod persistence;

#[cfg(test)]
mod tests;

pub use connectivity::{ChannelSettings, EmbeddingsSettings, HeartbeatSettings, TunnelSettings};
pub use execution::{
    AgentSettings, BuilderSettings, SafetySettings, SandboxSettings, TranscriptionSettings,
    WasmSettings,
};

use serde::{Deserialize, Serialize};

/// User settings persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    /// Whether onboarding wizard has been completed.
    #[serde(default, alias = "setup_completed")]
    pub onboard_completed: bool,

    // === Step 1: Database ===
    /// Database backend: "postgres" or "libsql".
    #[serde(default)]
    pub database_backend: Option<String>,

    /// Database connection URL (postgres://...).
    #[serde(default)]
    pub database_url: Option<String>,

    /// Database pool size.
    #[serde(default)]
    pub database_pool_size: Option<usize>,

    /// Path to local libSQL database file.
    #[serde(default)]
    pub libsql_path: Option<String>,

    /// Turso cloud URL for remote replica sync.
    #[serde(default)]
    pub libsql_url: Option<String>,

    // === Step 2: Security ===
    /// Source for the secrets master key.
    #[serde(default)]
    pub secrets_master_key_source: KeySource,

    /// Generated master key hex (env var mode only, written to .env by wizard).
    #[serde(default, skip_serializing)]
    pub secrets_master_key_hex: Option<String>,

    // === Step 3: Inference Provider ===
    /// LLM backend: "nearai", "anthropic", "openai", "ollama", "openai_compatible", "tinfoil", "bedrock".
    #[serde(default)]
    pub llm_backend: Option<String>,

    /// Ollama base URL (when llm_backend = "ollama").
    #[serde(default)]
    pub ollama_base_url: Option<String>,

    /// OpenAI-compatible endpoint base URL (when llm_backend = "openai_compatible").
    #[serde(default)]
    pub openai_compatible_base_url: Option<String>,

    /// Bedrock region (when llm_backend = "bedrock").
    #[serde(default)]
    pub bedrock_region: Option<String>,

    /// Bedrock cross-region inference prefix (when llm_backend = "bedrock").
    #[serde(default)]
    pub bedrock_cross_region: Option<String>,

    /// AWS profile name for Bedrock (when llm_backend = "bedrock").
    #[serde(default)]
    pub bedrock_profile: Option<String>,

    // === Step 4: Model Selection ===
    /// Currently selected model.
    #[serde(default)]
    pub selected_model: Option<String>,

    // === Step 5: Embeddings ===
    /// Embeddings configuration.
    #[serde(default)]
    pub embeddings: EmbeddingsSettings,

    // === Step 6: Channels ===
    /// Tunnel configuration for public webhook endpoints.
    #[serde(default)]
    pub tunnel: TunnelSettings,

    /// Channel configuration.
    #[serde(default)]
    pub channels: ChannelSettings,

    // === Step 7: Heartbeat ===
    /// Heartbeat configuration.
    #[serde(default)]
    pub heartbeat: HeartbeatSettings,

    // === Advanced Settings (not asked during setup, editable via CLI) ===
    /// Agent behaviour configuration.
    #[serde(default)]
    pub agent: AgentSettings,

    /// WASM sandbox configuration.
    #[serde(default)]
    pub wasm: WasmSettings,

    /// Docker sandbox configuration.
    #[serde(default)]
    pub sandbox: SandboxSettings,

    /// Safety configuration.
    #[serde(default)]
    pub safety: SafetySettings,

    /// Builder configuration.
    #[serde(default)]
    pub builder: BuilderSettings,

    /// Transcription configuration.
    #[serde(default)]
    pub transcription: Option<TranscriptionSettings>,
}

/// Source for the secrets master key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum KeySource {
    /// Auto-generated key stored in OS keychain.
    Keychain,
    /// User provides via SECRETS_MASTER_KEY env var.
    Env,
    /// Not configured (secrets features disabled).
    #[default]
    None,
}

fn default_true() -> bool {
    true
}
