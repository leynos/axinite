//! LLM configuration loading: backend selection, provider resolution, and
//! session settings drawn from the environment and persisted settings.

use std::path::PathBuf;

use crate::bootstrap::ironclaw_base_dir;
use crate::config::EnvContext;
use crate::config::helpers::{EnvKey, parse_optional_env_from};
use crate::error::ConfigError;
use crate::llm::config::*;
use crate::llm::registry::ProviderRegistry;
use crate::llm::session::SessionConfig;
use crate::settings::Settings;

#[path = "llm/provider_resolution.rs"]
mod provider_resolution;

impl LlmConfig {
    /// Create a test-friendly config without reading env vars.
    #[cfg(feature = "libsql")]
    pub fn for_testing() -> Self {
        Self {
            backend: "nearai".to_string(),
            session: SessionConfig {
                auth_base_url: "http://localhost:0".to_string(),
                session_path: std::env::temp_dir().join("ironclaw-test-session.json"),
            },
            nearai: NearAiConfig {
                model: "test-model".to_string(),
                cheap_model: None,
                base_url: "http://localhost:0".to_string(),
                api_key: None,
                fallback_model: None,
                max_retries: 0,
                circuit_breaker_threshold: None,
                circuit_breaker_recovery_secs: 30,
                response_cache_enabled: false,
                response_cache_ttl_secs: 3600,
                response_cache_max_entries: 100,
                failover_cooldown_secs: 300,
                failover_cooldown_threshold: 3,
                smart_routing_cascade: false,
            },
            provider: None,
            bedrock: None,
            request_timeout_secs: 120,
        }
    }

    // Backwards-compatible ambient entrypoint retained for existing callers.
    pub(crate) fn resolve(settings: &Settings) -> Result<Self, ConfigError> {
        Self::resolve_from(&EnvContext::capture_ambient(), settings)
    }

    pub(crate) fn resolve_from(ctx: &EnvContext, settings: &Settings) -> Result<Self, ConfigError> {
        let registry = ProviderRegistry::load()?;
        let (backend_lower, is_nearai, is_bedrock) = Self::resolve_backend_name(ctx, settings)?;
        let session = Self::resolve_session_config(ctx)?;
        let nearai = Self::resolve_nearai_config(ctx, settings)?;
        let provider = if is_nearai || is_bedrock {
            None
        } else {
            Some(Self::resolve_registry_provider(
                ctx,
                &backend_lower,
                &registry,
                settings,
            )?)
        };
        let bedrock = Self::resolve_bedrock_config(ctx, settings, is_bedrock)?;
        let request_timeout_secs =
            parse_optional_env_from(ctx, EnvKey("LLM_REQUEST_TIMEOUT_SECS"), 120)?;

        Ok(Self {
            backend: Self::backend_tag(is_nearai, is_bedrock, provider.as_ref(), backend_lower),
            session,
            nearai,
            provider,
            bedrock,
            request_timeout_secs,
        })
    }

    /// Choose the backend identifier stored on the config.
    ///
    /// Built-in backends keep their canonical tags; registry providers use
    /// their provider id; otherwise the lowered backend name stands as-is.
    fn backend_tag(
        is_nearai: bool,
        is_bedrock: bool,
        provider: Option<&RegistryProviderConfig>,
        backend_lower: String,
    ) -> String {
        if is_nearai {
            "nearai".to_string()
        } else if is_bedrock {
            "bedrock".to_string()
        } else if let Some(p) = provider {
            p.provider_id.clone()
        } else {
            backend_lower
        }
    }
}

/// Parse one `Key:Value` header entry, trimming whitespace from both sides.
fn parse_header_pair(pair: &str) -> Result<(String, String), ConfigError> {
    let Some((key, value)) = pair.split_once(':') else {
        return Err(ConfigError::InvalidValue {
            key: "LLM_EXTRA_HEADERS".to_string(),
            message: format!("malformed header entry '{}', expected Key:Value", pair),
        });
    };
    let key = key.trim();
    if key.is_empty() {
        return Err(ConfigError::InvalidValue {
            key: "LLM_EXTRA_HEADERS".to_string(),
            message: format!("empty header name in entry '{}'", pair),
        });
    }
    Ok((key.to_string(), value.trim().to_string()))
}

/// Parse `LLM_EXTRA_HEADERS` value into a list of (key, value) pairs.
///
/// Format: `Key1:Value1,Key2:Value2` (colon-separated, not `=`, because
/// header values often contain `=`).
fn parse_extra_headers(val: &str) -> Result<Vec<(String, String)>, ConfigError> {
    if val.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut headers = Vec::new();
    for pair in val.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        headers.push(parse_header_pair(pair)?);
    }
    Ok(headers)
}

/// Get the default session file path (~/.ironclaw/session.json).
pub fn default_session_path() -> PathBuf {
    ironclaw_base_dir().join("session.json")
}

#[cfg(test)]
mod tests;
