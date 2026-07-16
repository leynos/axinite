//! Declarative LLM provider registry.
//!
//! Providers are defined in JSON (compiled-in defaults + optional user file)
//! so adding a new OpenAI-compatible provider requires zero Rust code changes.
//!
//! ```text
//!   ┌─────────────────────┐    ┌──────────────────────────┐
//!   │  providers.json     │    │ ~/.ironclaw/providers.json│
//!   │  (built-in, embed)  │    │ (user overrides/extras)  │
//!   └────────┬────────────┘    └────────────┬─────────────┘
//!            │                              │
//!            └──────────┬───────────────────┘
//!                       ▼
//!              ┌──────────────────┐
//!              │ ProviderRegistry │
//!              │  .find("groq")   │──▶ ProviderDefinition
//!              │  .all()          │        ├ protocol
//!              │  .selectable()   │        ├ default_base_url
//!              └──────────────────┘        ├ api_key_env
//!                                          └ ...
//! ```

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// API protocol a provider speaks.
///
/// Determines which rig-core client constructor to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProtocol {
    /// OpenAI Chat Completions API (`/v1/chat/completions`).
    /// Used by: OpenAI, Tinfoil, Groq, NVIDIA NIM, OpenRouter, etc.
    OpenAiCompletions,
    /// Anthropic Messages API.
    Anthropic,
    /// Ollama API (OpenAI-ish, no API key required).
    Ollama,
}

/// How the setup wizard should collect credentials for this provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SetupHint {
    /// Collect an API key and store it in the encrypted secrets store.
    ApiKey {
        /// Key name in the secrets store (e.g., "llm_groq_api_key").
        secret_name: String,
        /// URL where the user can generate an API key.
        #[serde(default)]
        key_url: Option<String>,
        /// Human-readable name for display in the wizard.
        display_name: String,
        /// Whether this provider supports `/v1/models` listing.
        #[serde(default)]
        can_list_models: bool,
        /// Optional filter for model listing (e.g., "chat").
        #[serde(default)]
        models_filter: Option<String>,
    },
    /// Ollama-style setup: just a base URL, no API key.
    Ollama {
        display_name: String,
        #[serde(default)]
        can_list_models: bool,
    },
    /// Generic OpenAI-compatible: ask for base URL + optional API key.
    OpenAiCompatible {
        secret_name: String,
        display_name: String,
        #[serde(default)]
        can_list_models: bool,
    },
}

impl SetupHint {
    pub fn display_name(&self) -> &str {
        match self {
            Self::ApiKey { display_name, .. } => display_name,
            Self::Ollama { display_name, .. } => display_name,
            Self::OpenAiCompatible { display_name, .. } => display_name,
        }
    }

    pub fn can_list_models(&self) -> bool {
        match self {
            Self::ApiKey {
                can_list_models, ..
            } => *can_list_models,
            Self::Ollama {
                can_list_models, ..
            } => *can_list_models,
            Self::OpenAiCompatible {
                can_list_models, ..
            } => *can_list_models,
        }
    }

    pub fn secret_name(&self) -> Option<&str> {
        match self {
            Self::ApiKey { secret_name, .. } => Some(secret_name),
            Self::OpenAiCompatible { secret_name, .. } => Some(secret_name),
            Self::Ollama { .. } => None,
        }
    }

    pub fn models_filter(&self) -> Option<&str> {
        match self {
            Self::ApiKey { models_filter, .. } => models_filter.as_deref(),
            _ => None,
        }
    }
}

mod unsupported_params_de {
    //! Validates `unsupported_params` during deserialization.
    //!
    //! Only allows: "temperature", "max_tokens", "stop_sequences".
    //! Invalid parameter names cause a deserialization error.

    use serde::{Deserialize, Deserializer};

    const VALID_PARAMS: &[&str] = &["temperature", "max_tokens", "stop_sequences"];

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let params: Vec<String> = Deserialize::deserialize(deserializer)?;
        for param in &params {
            if !VALID_PARAMS.contains(&param.as_str()) {
                return Err(serde::de::Error::custom(format!(
                    "unsupported parameter name '{}': must be one of: {}",
                    param,
                    VALID_PARAMS.join(", ")
                )));
            }
        }
        Ok(params)
    }
}

/// Declarative definition of an LLM provider.
///
/// One JSON object in `providers.json` maps to one `ProviderDefinition`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDefinition {
    /// Unique identifier used in `LLM_BACKEND` (e.g., "groq", "tinfoil").
    pub id: String,
    /// Alternative names accepted in `LLM_BACKEND` (e.g., ["nvidia_nim", "nim"]).
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Which API protocol to use.
    pub protocol: ProviderProtocol,
    /// Default base URL. `None` means use the rig-core default for the protocol.
    #[serde(default)]
    pub default_base_url: Option<String>,
    /// Env var for base URL override (e.g., "OPENAI_BASE_URL").
    #[serde(default)]
    pub base_url_env: Option<String>,
    /// Whether a base URL is required (for generic openai_compatible).
    #[serde(default)]
    pub base_url_required: bool,
    /// Env var for the API key (e.g., "GROQ_API_KEY").
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Whether an API key is required to use this provider.
    #[serde(default)]
    pub api_key_required: bool,
    /// Env var for the model name (e.g., "GROQ_MODEL").
    pub model_env: String,
    /// Default model if none specified.
    pub default_model: String,
    /// Human-readable one-line description.
    pub description: String,
    /// Env var for extra HTTP headers (format: `Key:Value,Key2:Value2`).
    #[serde(default)]
    pub extra_headers_env: Option<String>,
    /// Setup wizard hints.
    #[serde(default)]
    pub setup: Option<SetupHint>,
    /// Parameter names that this provider does not support (e.g., `["temperature"]`).
    /// Supported keys: `"temperature"`, `"max_tokens"`, `"stop_sequences"`.
    /// Listed parameters are stripped from requests before sending to avoid 400 errors.
    /// Invalid parameter names cause a deserialization error.
    #[serde(default, deserialize_with = "unsupported_params_de::deserialize")]
    pub unsupported_params: Vec<String>,
}

/// Registry of known LLM providers.
///
/// Built from compiled-in `providers.json` plus optional user overrides
/// from `~/.ironclaw/providers.json`.
pub struct ProviderRegistry {
    providers: Vec<ProviderDefinition>,
    /// Lowercase id/alias → index into `providers`.
    lookup: HashMap<String, usize>,
}

impl ProviderRegistry {
    /// Build a registry from a list of provider definitions.
    ///
    /// Later entries with duplicate IDs/aliases override earlier ones.
    pub fn new(providers: Vec<ProviderDefinition>) -> Self {
        let mut lookup = HashMap::new();
        for (idx, def) in providers.iter().enumerate() {
            lookup.insert(def.id.to_lowercase(), idx);
            for alias in &def.aliases {
                lookup.insert(alias.to_lowercase(), idx);
            }
        }
        Self { providers, lookup }
    }

    /// Load the default registry: built-in providers + user overrides.
    ///
    /// User providers from `~/.ironclaw/providers.json` are appended,
    /// with later entries overriding earlier ones by ID/alias.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::ParseError`] when the compiled-in
    /// `providers.json` is not valid JSON. Invalid user overrides are
    /// logged and skipped rather than treated as fatal.
    pub fn load() -> Result<Self, crate::error::ConfigError> {
        let builtins: Vec<ProviderDefinition> =
            serde_json::from_str(include_str!("../../providers.json")).map_err(|e| {
                crate::error::ConfigError::ParseError(format!(
                    "built-in providers.json must be valid JSON: {e}"
                ))
            })?;

        let mut all = builtins;

        if let Some(user_path) = user_providers_path()
            && user_path.exists()
        {
            match ambient_fs::read_to_string(&user_path) {
                Ok(contents) => match serde_json::from_str::<Vec<ProviderDefinition>>(&contents) {
                    Ok(user_defs) => {
                        tracing::info!(
                            count = user_defs.len(),
                            path = %user_path.display(),
                            "Loaded user provider definitions"
                        );
                        all.extend(user_defs);
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %user_path.display(),
                            error = %e,
                            "Failed to parse user providers.json, skipping"
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        path = %user_path.display(),
                        error = %e,
                        "Failed to read user providers.json, skipping"
                    );
                }
            }
        }

        Ok(Self::new(all))
    }

    /// Look up a provider by ID or alias (case-insensitive).
    pub fn find(&self, id: &str) -> Option<&ProviderDefinition> {
        self.lookup
            .get(&id.to_lowercase())
            .map(|&idx| &self.providers[idx])
    }

    /// All registered providers (built-in + user).
    pub fn all(&self) -> &[ProviderDefinition] {
        &self.providers
    }

    /// Providers that should appear in the setup wizard's selection menu.
    ///
    /// Returns all providers that have a `setup` hint, in registry order.
    /// NearAI is not in the registry (handled specially) so it won't appear here.
    pub fn selectable(&self) -> Vec<&ProviderDefinition> {
        // Deduplicate: only keep the last definition for each ID
        let mut seen = HashMap::new();
        for def in &self.providers {
            seen.insert(def.id.as_str(), def);
        }
        // Preserve order of first appearance, but use the last (overridden)
        // definition for each ID. A user override that adds `setup` to a
        // provider that previously lacked it will be included correctly.
        let mut result = Vec::new();
        let mut emitted = std::collections::HashSet::new();
        for def in &self.providers {
            if emitted.insert(def.id.as_str()) {
                let final_def = seen[def.id.as_str()];
                if final_def.setup.is_some() {
                    result.push(final_def);
                }
            }
        }
        result
    }

    /// Whether the backend string names the NearAI provider.
    fn is_nearai_backend(backend: &str) -> bool {
        matches!(backend, "nearai" | "near_ai" | "near")
    }

    /// Check whether a backend string is a known provider (NearAI or registry).
    pub fn is_known(&self, backend: &str) -> bool {
        Self::is_nearai_backend(backend) || self.find(backend).is_some()
    }

    /// Get the model env var for a backend string.
    ///
    /// Returns the registry provider's `model_env` if found,
    /// or `"NEARAI_MODEL"` for the NearAI backend.
    pub fn model_env_var(&self, backend: &str) -> &str {
        if Self::is_nearai_backend(backend) {
            return "NEARAI_MODEL";
        }
        self.find(backend)
            .map(|def| def.model_env.as_str())
            .unwrap_or("LLM_MODEL")
    }
}

fn user_providers_path() -> Option<std::path::PathBuf> {
    Some(crate::bootstrap::ironclaw_base_dir().join("providers.json"))
}

#[cfg(test)]
mod tests;
