//! Helpers for resolving provider-specific LLM configuration from settings and
//! explicit environment snapshots.

use std::path::PathBuf;

use secrecy::SecretString;

use super::parse_extra_headers;
use crate::config::EnvContext;
use crate::config::helpers::{EnvKey, optional_env_from, parse_optional_env_from};
use crate::error::ConfigError;
use crate::llm::config::*;
use crate::llm::registry::{ProviderProtocol, ProviderRegistry};
use crate::llm::session::SessionConfig;
use crate::settings::Settings;

fn resolve_api_key(
    ctx: &EnvContext,
    api_key_env: Option<&str>,
    api_key_required: bool,
    backend: &str,
) -> Result<Option<SecretString>, ConfigError> {
    let key = if let Some(env_var) = api_key_env {
        ctx.get_owned(env_var).map(SecretString::from)
    } else {
        None
    };
    let key_required_but_missing = api_key_required && key.is_none();
    if key_required_but_missing {
        let Some(env_var) = api_key_env else {
            return Ok(key);
        };
        tracing::debug!(
            "API key not found in {env_var} for backend '{backend}'. \
             Will be injected from secrets store if available."
        );
    }
    Ok(key)
}

/// Provider-definition inputs for base-URL resolution.
struct BaseUrlSpec<'a> {
    env_var: Option<&'a str>,
    backend: &'a str,
    default: Option<&'a str>,
    required: bool,
}

fn backend_name(ctx: &EnvContext, settings: &Settings) -> Result<String, ConfigError> {
    Ok(
        if let Some(backend) = optional_env_from(ctx, EnvKey("LLM_BACKEND"))? {
            backend
        } else if let Some(backend) = &settings.llm_backend {
            backend.clone()
        } else {
            "nearai".to_string()
        },
    )
}

fn is_nearai_backend(backend: &str) -> bool {
    matches!(backend, "nearai" | "near_ai" | "near")
}

fn is_bedrock_backend(backend: &str) -> bool {
    matches!(backend, "bedrock" | "aws_bedrock" | "aws")
}

fn should_warn_unknown_backend(
    backend: &str,
    is_nearai: bool,
    is_bedrock: bool,
) -> Result<bool, ConfigError> {
    Ok(!is_nearai && !is_bedrock && ProviderRegistry::load()?.find(backend).is_none())
}

fn resolve_bedrock_region(ctx: &EnvContext, settings: &Settings) -> Result<String, ConfigError> {
    let explicit = optional_env_from(ctx, EnvKey("BEDROCK_REGION"))?
        .or_else(|| settings.bedrock_region.clone());
    if explicit.is_none() {
        tracing::info!("BEDROCK_REGION not set, defaulting to us-east-1");
    }
    Ok(explicit.unwrap_or_else(|| "us-east-1".to_string()))
}

fn resolve_bedrock_model(ctx: &EnvContext, settings: &Settings) -> Result<String, ConfigError> {
    optional_env_from(ctx, EnvKey("BEDROCK_MODEL"))?
        .or_else(|| settings.selected_model.clone())
        .ok_or_else(|| ConfigError::MissingRequired {
            key: "BEDROCK_MODEL".to_string(),
            hint: "Set BEDROCK_MODEL when LLM_BACKEND=bedrock".to_string(),
        })
}

fn resolve_base_url(
    ctx: &EnvContext,
    spec: &BaseUrlSpec<'_>,
    settings: &Settings,
) -> Result<String, ConfigError> {
    let base_url = if let Some(env_var) = spec.env_var {
        ctx.get_owned(env_var)
    } else {
        None
    }
    .or_else(|| match spec.backend {
        "ollama" => settings.ollama_base_url.clone(),
        "openai_compatible" | "openrouter" => settings.openai_compatible_base_url.clone(),
        _ => None,
    })
    .or_else(|| spec.default.map(String::from))
    .unwrap_or_default();

    if let Some(env_var) = spec.env_var {
        let url_required_but_absent = spec.required && base_url.is_empty();
        if url_required_but_absent {
            return Err(ConfigError::MissingRequired {
                key: env_var.to_string(),
                hint: format!("Set {env_var} when LLM_BACKEND={}", spec.backend),
            });
        }
    }
    Ok(base_url)
}

fn resolve_extra_headers(
    ctx: &EnvContext,
    extra_headers_env: Option<&str>,
) -> Result<Vec<(String, String)>, ConfigError> {
    let headers = if let Some(env_var) = extra_headers_env {
        ctx.get_owned(env_var)
            .map(|val| parse_extra_headers(&val))
            .transpose()?
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    Ok(headers)
}

fn resolve_anthropic_credentials(
    ctx: &EnvContext,
    canonical_id: &str,
    api_key: Option<SecretString>,
) -> Result<(Option<SecretString>, Option<SecretString>), ConfigError> {
    let oauth_token = if canonical_id == "anthropic" {
        optional_env_from(ctx, EnvKey("ANTHROPIC_OAUTH_TOKEN"))?.map(SecretString::from)
    } else {
        None
    };
    let api_key = if api_key.is_none() && oauth_token.is_some() {
        Some(SecretString::from(OAUTH_PLACEHOLDER.to_string()))
    } else {
        api_key
    };
    Ok((api_key, oauth_token))
}

fn resolve_provider_cache_retention(
    ctx: &EnvContext,
    canonical_id: &str,
) -> Result<CacheRetention, ConfigError> {
    if canonical_id != "anthropic" {
        return Ok(CacheRetention::default());
    }
    Ok(optional_env_from(ctx, EnvKey("ANTHROPIC_CACHE_RETENTION"))?
        .and_then(|val| match val.parse::<CacheRetention>() {
            Ok(r) => Some(r),
            Err(e) => {
                tracing::warn!("Invalid ANTHROPIC_CACHE_RETENTION: {e}; defaulting to short");
                None
            }
        })
        .unwrap_or_default())
}

fn validate_bedrock_cross_region(cross_region: &Option<String>) -> Result<(), ConfigError> {
    if let Some(cr) = cross_region
        && !matches!(cr.as_str(), "us" | "eu" | "apac" | "global")
    {
        return Err(ConfigError::InvalidValue {
            key: "BEDROCK_CROSS_REGION".to_string(),
            message: format!(
                "'{}' is not valid, expected one of: us, eu, apac, global",
                cr
            ),
        });
    }
    Ok(())
}

struct ProviderKeySpec<'a> {
    canonical_id: &'a str,
    api_key_env: Option<&'a str>,
    api_key_required: bool,
    backend: &'a str,
}

/// Owned provider definition resolved from the registry or synthesised as a fallback.
struct ProviderSpec {
    canonical_id: String,
    protocol: ProviderProtocol,
    api_key_env: Option<String>,
    base_url_env: Option<String>,
    model_env: String,
    default_model: String,
    default_base_url: Option<String>,
    extra_headers_env: Option<String>,
    api_key_required: bool,
    base_url_required: bool,
    unsupported_params: Vec<String>,
}

#[path = "provider_resolution/llm_config.rs"]
mod llm_config;
