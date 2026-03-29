use std::path::PathBuf;

use secrecy::SecretString;

use crate::bootstrap::ironclaw_base_dir;
use crate::config::EnvContext;
use crate::config::helpers::{optional_env_from, parse_optional_env_from};
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
        optional_env_from(ctx, env_var)?.map(SecretString::from)
    } else {
        None
    };
    if api_key_required
        && key.is_none()
        && let Some(env_var) = api_key_env
    {
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

fn resolve_base_url(
    ctx: &EnvContext,
    spec: &BaseUrlSpec<'_>,
    settings: &Settings,
) -> Result<String, ConfigError> {
    let base_url = if let Some(env_var) = spec.env_var {
        optional_env_from(ctx, env_var)?
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

    if spec.required
        && base_url.is_empty()
        && let Some(env_var) = spec.env_var
    {
        return Err(ConfigError::MissingRequired {
            key: env_var.to_string(),
            hint: format!("Set {env_var} when LLM_BACKEND={}", spec.backend),
        });
    }
    Ok(base_url)
}

fn resolve_extra_headers(
    ctx: &EnvContext,
    extra_headers_env: Option<&str>,
) -> Result<Vec<(String, String)>, ConfigError> {
    let headers = if let Some(env_var) = extra_headers_env {
        optional_env_from(ctx, env_var)?
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
        optional_env_from(ctx, "ANTHROPIC_OAUTH_TOKEN")?.map(SecretString::from)
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
    Ok(optional_env_from(ctx, "ANTHROPIC_CACHE_RETENTION")?
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

    /// Resolve a model name from env var -> settings.selected_model -> hardcoded default.
    fn resolve_model(
        ctx: &EnvContext,
        env_var: &str,
        settings: &Settings,
        default: &str,
    ) -> Result<String, ConfigError> {
        Ok(optional_env_from(ctx, env_var)?
            .or_else(|| settings.selected_model.clone())
            .unwrap_or_else(|| default.to_string()))
    }

    fn resolve_provider_credentials(
        ctx: &EnvContext,
        spec: &ProviderKeySpec<'_>,
    ) -> Result<(Option<SecretString>, Option<SecretString>), ConfigError> {
        let api_key = resolve_api_key(ctx, spec.api_key_env, spec.api_key_required, spec.backend)?;
        resolve_anthropic_credentials(ctx, spec.canonical_id, api_key)
    }

    fn resolve_provider_spec(backend: &str, registry: &ProviderRegistry) -> ProviderSpec {
        if let Some(def) = registry
            .find(backend)
            .or_else(|| registry.find("openai_compatible"))
        {
            ProviderSpec {
                canonical_id: def.id.clone(),
                protocol: def.protocol,
                api_key_env: def.api_key_env.clone(),
                base_url_env: def.base_url_env.clone(),
                model_env: def.model_env.clone(),
                default_model: def.default_model.clone(),
                default_base_url: def.default_base_url.clone(),
                extra_headers_env: def.extra_headers_env.clone(),
                api_key_required: def.api_key_required,
                base_url_required: def.base_url_required,
                unsupported_params: def.unsupported_params.clone(),
            }
        } else {
            ProviderSpec {
                canonical_id: backend.to_string(),
                protocol: ProviderProtocol::OpenAiCompletions,
                api_key_env: Some("LLM_API_KEY".to_string()),
                base_url_env: Some("LLM_BASE_URL".to_string()),
                model_env: "LLM_MODEL".to_string(),
                default_model: "default".to_string(),
                default_base_url: None,
                extra_headers_env: Some("LLM_EXTRA_HEADERS".to_string()),
                api_key_required: false,
                base_url_required: true,
                unsupported_params: Vec::new(),
            }
        }
    }

    fn resolve_backend_name(
        ctx: &EnvContext,
        settings: &Settings,
    ) -> Result<(String, bool, bool), ConfigError> {
        let backend = if let Some(b) = optional_env_from(ctx, "LLM_BACKEND")? {
            b
        } else if let Some(ref b) = settings.llm_backend {
            b.clone()
        } else {
            "nearai".to_string()
        };

        let backend_lower = backend.to_lowercase();
        let is_nearai =
            backend_lower == "nearai" || backend_lower == "near_ai" || backend_lower == "near";
        let is_bedrock =
            backend_lower == "bedrock" || backend_lower == "aws_bedrock" || backend_lower == "aws";

        if !is_nearai && !is_bedrock {
            let registry = ProviderRegistry::load();
            if registry.find(&backend_lower).is_none() {
                tracing::warn!(
                    "Unknown LLM backend '{}'. Will attempt as openai_compatible fallback.",
                    backend
                );
            }
        }

        Ok((backend_lower, is_nearai, is_bedrock))
    }

    fn resolve_session_config(ctx: &EnvContext) -> Result<SessionConfig, ConfigError> {
        Ok(SessionConfig {
            auth_base_url: optional_env_from(ctx, "NEARAI_AUTH_URL")?
                .unwrap_or_else(|| "https://private.near.ai".to_string()),
            session_path: optional_env_from(ctx, "NEARAI_SESSION_PATH")?
                .map(PathBuf::from)
                .unwrap_or_else(|| ctx.ironclaw_base_dir().join("session.json")),
        })
    }

    fn resolve_nearai_config(
        ctx: &EnvContext,
        settings: &Settings,
    ) -> Result<NearAiConfig, ConfigError> {
        let nearai_api_key = optional_env_from(ctx, "NEARAI_API_KEY")?.map(SecretString::from);
        Ok(NearAiConfig {
            model: Self::resolve_model(ctx, "NEARAI_MODEL", settings, "zai-org/GLM-latest")?,
            cheap_model: optional_env_from(ctx, "NEARAI_CHEAP_MODEL")?,
            base_url: optional_env_from(ctx, "NEARAI_BASE_URL")?.unwrap_or_else(|| {
                if nearai_api_key.is_some() {
                    "https://cloud-api.near.ai".to_string()
                } else {
                    "https://private.near.ai".to_string()
                }
            }),
            api_key: nearai_api_key,
            fallback_model: optional_env_from(ctx, "NEARAI_FALLBACK_MODEL")?,
            max_retries: parse_optional_env_from(ctx, "NEARAI_MAX_RETRIES", 3)?,
            circuit_breaker_threshold: optional_env_from(ctx, "CIRCUIT_BREAKER_THRESHOLD")?
                .map(|s| s.parse())
                .transpose()
                .map_err(|e| ConfigError::InvalidValue {
                    key: "CIRCUIT_BREAKER_THRESHOLD".to_string(),
                    message: format!("must be a positive integer: {e}"),
                })?,
            circuit_breaker_recovery_secs: parse_optional_env_from(
                ctx,
                "CIRCUIT_BREAKER_RECOVERY_SECS",
                30,
            )?,
            response_cache_enabled: parse_optional_env_from(ctx, "RESPONSE_CACHE_ENABLED", false)?,
            response_cache_ttl_secs: parse_optional_env_from(ctx, "RESPONSE_CACHE_TTL_SECS", 3600)?,
            response_cache_max_entries: parse_optional_env_from(
                ctx,
                "RESPONSE_CACHE_MAX_ENTRIES",
                1000,
            )?,
            failover_cooldown_secs: parse_optional_env_from(
                ctx,
                "LLM_FAILOVER_COOLDOWN_SECS",
                300,
            )?,
            failover_cooldown_threshold: parse_optional_env_from(ctx, "LLM_FAILOVER_THRESHOLD", 3)?,
            smart_routing_cascade: parse_optional_env_from(ctx, "SMART_ROUTING_CASCADE", true)?,
        })
    }

    fn resolve_bedrock_config(
        ctx: &EnvContext,
        settings: &Settings,
        is_bedrock: bool,
    ) -> Result<Option<BedrockConfig>, ConfigError> {
        if !is_bedrock {
            return Ok(None);
        }
        let explicit_region =
            optional_env_from(ctx, "BEDROCK_REGION")?.or_else(|| settings.bedrock_region.clone());
        if explicit_region.is_none() {
            tracing::info!("BEDROCK_REGION not set, defaulting to us-east-1");
        }
        let region = explicit_region.unwrap_or_else(|| "us-east-1".to_string());
        let model = optional_env_from(ctx, "BEDROCK_MODEL")?
            .or_else(|| settings.selected_model.clone())
            .ok_or_else(|| ConfigError::MissingRequired {
                key: "BEDROCK_MODEL".to_string(),
                hint: "Set BEDROCK_MODEL when LLM_BACKEND=bedrock".to_string(),
            })?;
        let cross_region = optional_env_from(ctx, "BEDROCK_CROSS_REGION")?
            .or_else(|| settings.bedrock_cross_region.clone());
        validate_bedrock_cross_region(&cross_region)?;
        let profile =
            optional_env_from(ctx, "AWS_PROFILE")?.or_else(|| settings.bedrock_profile.clone());
        Ok(Some(BedrockConfig {
            region,
            model,
            cross_region,
            profile,
        }))
    }

    // Backwards-compatible ambient entrypoint retained for existing callers.
    #[allow(dead_code)]
    pub(crate) fn resolve(settings: &Settings) -> Result<Self, ConfigError> {
        Self::resolve_from(&EnvContext::capture_ambient(), settings)
    }

    pub(crate) fn resolve_from(ctx: &EnvContext, settings: &Settings) -> Result<Self, ConfigError> {
        let registry = ProviderRegistry::load();
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
        let request_timeout_secs = parse_optional_env_from(ctx, "LLM_REQUEST_TIMEOUT_SECS", 120)?;

        Ok(Self {
            backend: if is_nearai {
                "nearai".to_string()
            } else if is_bedrock {
                "bedrock".to_string()
            } else if let Some(ref p) = provider {
                p.provider_id.clone()
            } else {
                backend_lower
            },
            session,
            nearai,
            provider,
            bedrock,
            request_timeout_secs,
        })
    }

    /// Resolve a `RegistryProviderConfig` from the registry and env vars.
    fn resolve_registry_provider(
        ctx: &EnvContext,
        backend: &str,
        registry: &ProviderRegistry,
        settings: &Settings,
    ) -> Result<RegistryProviderConfig, ConfigError> {
        let spec = Self::resolve_provider_spec(backend, registry);

        let (api_key, oauth_token) = Self::resolve_provider_credentials(
            ctx,
            &ProviderKeySpec {
                canonical_id: &spec.canonical_id,
                api_key_env: spec.api_key_env.as_deref(),
                api_key_required: spec.api_key_required,
                backend,
            },
        )?;
        let base_url = resolve_base_url(
            ctx,
            &BaseUrlSpec {
                env_var: spec.base_url_env.as_deref(),
                backend,
                default: spec.default_base_url.as_deref(),
                required: spec.base_url_required,
            },
            settings,
        )?;
        let model = Self::resolve_model(ctx, &spec.model_env, settings, &spec.default_model)?;
        let extra_headers = resolve_extra_headers(ctx, spec.extra_headers_env.as_deref())?;
        let cache_retention = resolve_provider_cache_retention(ctx, &spec.canonical_id)?;

        Ok(RegistryProviderConfig {
            protocol: spec.protocol,
            provider_id: spec.canonical_id,
            api_key,
            base_url,
            model,
            extra_headers,
            oauth_token,
            cache_retention,
            unsupported_params: spec.unsupported_params,
        })
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
mod tests {
    use super::*;
    use crate::config::helpers::ENV_MUTEX;
    use crate::settings::Settings;
    use crate::testing::credentials::*;

    /// Clear all openai-compatible-related env vars.
    fn clear_openai_compatible_env() {
        // SAFETY: Only called under ENV_MUTEX in tests.
        unsafe {
            std::env::remove_var("LLM_BACKEND");
            std::env::remove_var("LLM_BASE_URL");
            std::env::remove_var("LLM_MODEL");
        }
    }

    #[test]
    fn openai_compatible_uses_selected_model_when_llm_model_unset() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_openai_compatible_env();

        let settings = Settings {
            llm_backend: Some("openai_compatible".to_string()),
            openai_compatible_base_url: Some("https://openrouter.ai/api/v1".to_string()),
            selected_model: Some("openai/gpt-5.1-codex".to_string()),
            ..Default::default()
        };

        let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
        let provider = cfg.provider.expect("provider config should be present");

        assert_eq!(provider.model, "openai/gpt-5.1-codex");
    }

    #[test]
    fn openai_compatible_llm_model_env_overrides_selected_model() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_openai_compatible_env();
        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::set_var("LLM_MODEL", "openai/gpt-5-codex");
        }

        let settings = Settings {
            llm_backend: Some("openai_compatible".to_string()),
            openai_compatible_base_url: Some("https://openrouter.ai/api/v1".to_string()),
            selected_model: Some("openai/gpt-5.1-codex".to_string()),
            ..Default::default()
        };

        let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
        let provider = cfg.provider.expect("provider config should be present");

        assert_eq!(provider.model, "openai/gpt-5-codex");

        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::remove_var("LLM_MODEL");
        }
    }

    #[test]
    fn test_extra_headers_parsed() {
        let result = parse_extra_headers("HTTP-Referer:https://myapp.com,X-Title:MyApp").unwrap();
        assert_eq!(
            result,
            vec![
                ("HTTP-Referer".to_string(), "https://myapp.com".to_string()),
                ("X-Title".to_string(), "MyApp".to_string()),
            ]
        );
    }

    #[test]
    fn test_extra_headers_empty_string() {
        let result = parse_extra_headers("").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_extra_headers_whitespace_only() {
        let result = parse_extra_headers("  ").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_extra_headers_malformed() {
        let result = parse_extra_headers("NoColonHere");
        assert!(result.is_err());
    }

    #[test]
    fn test_extra_headers_empty_key() {
        let result = parse_extra_headers(":value");
        assert!(result.is_err());
    }

    #[test]
    fn test_extra_headers_value_with_colons() {
        let result = parse_extra_headers("Authorization:Bearer abc:def").unwrap();
        assert_eq!(
            result,
            vec![("Authorization".to_string(), "Bearer abc:def".to_string())]
        );
    }

    #[test]
    fn test_extra_headers_trailing_comma() {
        let result = parse_extra_headers("X-Title:MyApp,").unwrap();
        assert_eq!(result, vec![("X-Title".to_string(), "MyApp".to_string())]);
    }

    #[test]
    fn test_extra_headers_with_spaces() {
        let result =
            parse_extra_headers(" HTTP-Referer : https://myapp.com , X-Title : MyApp ").unwrap();
        assert_eq!(
            result,
            vec![
                ("HTTP-Referer".to_string(), "https://myapp.com".to_string()),
                ("X-Title".to_string(), "MyApp".to_string()),
            ]
        );
    }

    /// Clear all ollama-related env vars.
    fn clear_ollama_env() {
        // SAFETY: Only called under ENV_MUTEX in tests.
        unsafe {
            std::env::remove_var("LLM_BACKEND");
            std::env::remove_var("OLLAMA_BASE_URL");
            std::env::remove_var("OLLAMA_MODEL");
        }
    }

    #[test]
    fn ollama_uses_selected_model_when_ollama_model_unset() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_ollama_env();

        let settings = Settings {
            llm_backend: Some("ollama".to_string()),
            selected_model: Some("llama3.2".to_string()),
            ..Default::default()
        };

        let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
        let provider = cfg.provider.expect("provider config should be present");

        assert_eq!(provider.model, "llama3.2");
    }

    #[test]
    fn ollama_model_env_overrides_selected_model() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_ollama_env();
        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::set_var("OLLAMA_MODEL", "mistral:latest");
        }

        let settings = Settings {
            llm_backend: Some("ollama".to_string()),
            selected_model: Some("llama3.2".to_string()),
            ..Default::default()
        };

        let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
        let provider = cfg.provider.expect("provider config should be present");

        assert_eq!(provider.model, "mistral:latest");

        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::remove_var("OLLAMA_MODEL");
        }
    }

    #[test]
    fn openai_compatible_preserves_dotted_model_name() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_openai_compatible_env();

        let settings = Settings {
            llm_backend: Some("openai_compatible".to_string()),
            openai_compatible_base_url: Some("http://localhost:11434/v1".to_string()),
            selected_model: Some("llama3.2".to_string()),
            ..Default::default()
        };

        let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
        let provider = cfg.provider.expect("provider config should be present");

        assert_eq!(
            provider.model, "llama3.2",
            "model name with dot must not be truncated"
        );
    }

    #[test]
    fn registry_provider_resolves_groq() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::remove_var("LLM_BACKEND");
            std::env::remove_var("GROQ_API_KEY");
            std::env::remove_var("GROQ_MODEL");
        }

        let settings = Settings {
            llm_backend: Some("groq".to_string()),
            selected_model: Some("llama-3.3-70b-versatile".to_string()),
            ..Default::default()
        };

        let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
        assert_eq!(cfg.backend, "groq");
        let provider = cfg.provider.expect("provider config should be present");
        assert_eq!(provider.provider_id, "groq");
        assert_eq!(provider.model, "llama-3.3-70b-versatile");
        assert_eq!(provider.base_url, "https://api.groq.com/openai/v1");
        assert_eq!(provider.protocol, ProviderProtocol::OpenAiCompletions);
    }

    #[test]
    fn registry_provider_resolves_tinfoil() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::remove_var("LLM_BACKEND");
            std::env::remove_var("TINFOIL_API_KEY");
            std::env::remove_var("TINFOIL_MODEL");
        }

        let settings = Settings {
            llm_backend: Some("tinfoil".to_string()),
            ..Default::default()
        };

        let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
        assert_eq!(cfg.backend, "tinfoil");
        let provider = cfg.provider.expect("provider config should be present");
        assert_eq!(provider.base_url, "https://inference.tinfoil.sh/v1");
        assert_eq!(provider.model, "kimi-k2-5");
        assert!(
            provider
                .unsupported_params
                .contains(&"temperature".to_string()),
            "tinfoil should propagate unsupported_params from registry"
        );
    }

    #[test]
    fn nearai_backend_has_no_registry_provider() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::remove_var("LLM_BACKEND");
        }

        let settings = Settings::default();
        let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
        assert_eq!(cfg.backend, "nearai");
        assert!(cfg.provider.is_none());
    }

    #[test]
    fn backend_alias_normalized_to_canonical_id() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_openai_compatible_env();
        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::set_var("LLM_BACKEND", "open_ai");
            std::env::set_var("OPENAI_API_KEY", TEST_API_KEY);
        }

        let settings = Settings::default();
        let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
        assert_eq!(
            cfg.backend, "openai",
            "alias 'open_ai' should be normalized to canonical 'openai'"
        );
        let provider = cfg.provider.expect("should have provider config");
        assert_eq!(provider.provider_id, "openai");

        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::remove_var("LLM_BACKEND");
            std::env::remove_var("OPENAI_API_KEY");
        }
    }

    #[test]
    fn unknown_backend_falls_back_to_openai_compatible() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_openai_compatible_env();
        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::set_var("LLM_BACKEND", "some_custom_provider");
            std::env::set_var("LLM_BASE_URL", "http://localhost:8080/v1");
        }

        let settings = Settings::default();
        let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
        assert_eq!(cfg.backend, "openai_compatible");
        let provider = cfg.provider.expect("should have provider config");
        assert_eq!(provider.provider_id, "openai_compatible");
        assert_eq!(provider.base_url, "http://localhost:8080/v1");

        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::remove_var("LLM_BACKEND");
            std::env::remove_var("LLM_BASE_URL");
        }
    }

    #[test]
    fn nearai_aliases_all_resolve_to_nearai() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");

        for alias in &["nearai", "near_ai", "near"] {
            // SAFETY: Under ENV_MUTEX.
            unsafe {
                std::env::set_var("LLM_BACKEND", alias);
            }
            let settings = Settings::default();
            let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
            assert_eq!(
                cfg.backend, "nearai",
                "alias '{alias}' should resolve to 'nearai'"
            );
            assert!(
                cfg.provider.is_none(),
                "nearai should not have a registry provider"
            );
        }

        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::remove_var("LLM_BACKEND");
        }
    }

    #[test]
    fn base_url_resolution_priority() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_openai_compatible_env();

        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::set_var("LLM_BACKEND", "openai_compatible");
            std::env::set_var("LLM_BASE_URL", "http://env-url/v1");
        }

        let settings = Settings {
            llm_backend: Some("openai_compatible".to_string()),
            openai_compatible_base_url: Some("http://settings-url/v1".to_string()),
            ..Default::default()
        };

        let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
        let provider = cfg.provider.expect("should have provider config");
        assert_eq!(
            provider.base_url, "http://env-url/v1",
            "env var should take priority over settings"
        );

        // Now without env var, settings should win over registry default
        unsafe {
            std::env::remove_var("LLM_BASE_URL");
        }

        let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
        let provider = cfg.provider.expect("should have provider config");
        assert_eq!(
            provider.base_url, "http://settings-url/v1",
            "settings should take priority over registry default"
        );

        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::remove_var("LLM_BACKEND");
        }
    }

    // ── OAuth resolution tests ──────────────────────────────────────

    /// Clear all Anthropic-related env vars.
    fn clear_anthropic_env() {
        // SAFETY: Only called under ENV_MUTEX in tests.
        unsafe {
            std::env::remove_var("LLM_BACKEND");
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("ANTHROPIC_OAUTH_TOKEN");
            std::env::remove_var("ANTHROPIC_MODEL");
            std::env::remove_var("ANTHROPIC_BASE_URL");
        }
    }

    #[test]
    fn anthropic_oauth_token_sets_placeholder_api_key() {
        use secrecy::ExposeSecret;

        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_anthropic_env();
        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::set_var("ANTHROPIC_OAUTH_TOKEN", TEST_ANTHROPIC_OAUTH_TOKEN);
        }

        let settings = Settings {
            llm_backend: Some("anthropic".to_string()),
            ..Default::default()
        };
        let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
        let provider = cfg.provider.expect("provider config should be present");

        assert_eq!(
            provider
                .api_key
                .as_ref()
                .map(|k| k.expose_secret().to_string()),
            Some(OAUTH_PLACEHOLDER.to_string()),
            "api_key should be the OAuth placeholder when only OAuth token is set"
        );
        assert!(
            provider.oauth_token.is_some(),
            "oauth_token should be populated"
        );
        assert_eq!(
            provider.oauth_token.as_ref().unwrap().expose_secret(),
            TEST_ANTHROPIC_OAUTH_TOKEN
        );

        clear_anthropic_env();
    }

    #[test]
    fn anthropic_api_key_takes_priority_over_oauth() {
        use secrecy::ExposeSecret;

        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_anthropic_env();
        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::set_var("ANTHROPIC_API_KEY", TEST_ANTHROPIC_API_KEY);
            std::env::set_var("ANTHROPIC_OAUTH_TOKEN", TEST_ANTHROPIC_OAUTH_TOKEN);
        }

        let settings = Settings {
            llm_backend: Some("anthropic".to_string()),
            ..Default::default()
        };
        let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
        let provider = cfg.provider.expect("provider config should be present");

        assert_eq!(
            provider
                .api_key
                .as_ref()
                .map(|k| k.expose_secret().to_string()),
            Some(TEST_ANTHROPIC_API_KEY.to_string()),
            "real API key should take priority over OAuth placeholder"
        );
        assert!(
            provider.oauth_token.is_some(),
            "oauth_token should still be populated"
        );

        clear_anthropic_env();
    }

    #[test]
    fn non_anthropic_provider_has_no_oauth_token() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        clear_anthropic_env();
        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::set_var("ANTHROPIC_OAUTH_TOKEN", TEST_ANTHROPIC_OAUTH_TOKEN);
        }

        let settings = Settings {
            llm_backend: Some("openai".to_string()),
            ..Default::default()
        };
        let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
        let provider = cfg.provider.expect("provider config should be present");

        assert!(
            provider.oauth_token.is_none(),
            "non-Anthropic providers should not pick up ANTHROPIC_OAUTH_TOKEN"
        );

        clear_anthropic_env();
    }

    // ── Cache retention tests ───────────────────────────────────────

    #[test]
    fn cache_retention_from_str_primary_values() {
        assert_eq!(
            "none".parse::<CacheRetention>().unwrap(),
            CacheRetention::None
        );
        assert_eq!(
            "short".parse::<CacheRetention>().unwrap(),
            CacheRetention::Short
        );
        assert_eq!(
            "long".parse::<CacheRetention>().unwrap(),
            CacheRetention::Long
        );
    }

    #[test]
    fn cache_retention_from_str_aliases() {
        assert_eq!(
            "off".parse::<CacheRetention>().unwrap(),
            CacheRetention::None
        );
        assert_eq!(
            "disabled".parse::<CacheRetention>().unwrap(),
            CacheRetention::None
        );
        assert_eq!(
            "5m".parse::<CacheRetention>().unwrap(),
            CacheRetention::Short
        );
        assert_eq!(
            "ephemeral".parse::<CacheRetention>().unwrap(),
            CacheRetention::Short
        );
        assert_eq!(
            "1h".parse::<CacheRetention>().unwrap(),
            CacheRetention::Long
        );
    }

    #[test]
    fn cache_retention_from_str_case_insensitive() {
        assert_eq!(
            "NONE".parse::<CacheRetention>().unwrap(),
            CacheRetention::None
        );
        assert_eq!(
            "Short".parse::<CacheRetention>().unwrap(),
            CacheRetention::Short
        );
        assert_eq!(
            "LONG".parse::<CacheRetention>().unwrap(),
            CacheRetention::Long
        );
        assert_eq!(
            "Ephemeral".parse::<CacheRetention>().unwrap(),
            CacheRetention::Short
        );
    }

    #[test]
    fn cache_retention_from_str_invalid() {
        let err = "bogus".parse::<CacheRetention>().unwrap_err();
        assert!(
            err.contains("bogus"),
            "error should mention the invalid value"
        );
    }

    #[test]
    fn cache_retention_display_round_trip() {
        for variant in [
            CacheRetention::None,
            CacheRetention::Short,
            CacheRetention::Long,
        ] {
            let s = variant.to_string();
            let parsed: CacheRetention = s.parse().unwrap();
            assert_eq!(parsed, variant, "round-trip failed for {s}");
        }
    }

    #[test]
    fn test_request_timeout_defaults_to_120() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::remove_var("LLM_REQUEST_TIMEOUT_SECS");
        }
        let config = LlmConfig::resolve(&Settings::default()).expect("resolve");
        assert_eq!(config.request_timeout_secs, 120);
    }

    #[test]
    fn test_request_timeout_configurable() {
        let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::set_var("LLM_REQUEST_TIMEOUT_SECS", "300");
        }
        let config = LlmConfig::resolve(&Settings::default()).expect("resolve");
        assert_eq!(config.request_timeout_secs, 300);
        // SAFETY: Cleanup
        unsafe {
            std::env::remove_var("LLM_REQUEST_TIMEOUT_SECS");
        }
    }
}
