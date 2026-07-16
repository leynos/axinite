//! Associated resolution functions on [`LlmConfig`]: model, credential,
//! provider-spec, backend-name, session, NEAR AI, Bedrock, and registry
//! provider resolution built on the sibling helper functions.

use super::*;

impl LlmConfig {
    /// Resolve a model name from env var -> settings.selected_model -> hardcoded default.
    fn resolve_model(
        ctx: &EnvContext,
        env_var: &str,
        settings: &Settings,
        default: &str,
    ) -> Result<String, ConfigError> {
        Ok(ctx
            .get_owned(env_var)
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

    pub(in crate::config::llm) fn resolve_backend_name(
        ctx: &EnvContext,
        settings: &Settings,
    ) -> Result<(String, bool, bool), ConfigError> {
        let backend = backend_name(ctx, settings)?;
        let backend_lower = backend.to_lowercase();
        let is_nearai = is_nearai_backend(&backend_lower);
        let is_bedrock = is_bedrock_backend(&backend_lower);

        if should_warn_unknown_backend(&backend_lower, is_nearai, is_bedrock)? {
            tracing::warn!(
                "Unknown LLM backend '{}'. Will attempt as openai_compatible fallback.",
                backend
            );
        }

        Ok((backend_lower, is_nearai, is_bedrock))
    }

    pub(in crate::config::llm) fn resolve_session_config(
        ctx: &EnvContext,
    ) -> Result<SessionConfig, ConfigError> {
        Ok(SessionConfig {
            auth_base_url: optional_env_from(ctx, EnvKey("NEARAI_AUTH_URL"))?
                .unwrap_or_else(|| "https://private.near.ai".to_string()),
            session_path: optional_env_from(ctx, EnvKey("NEARAI_SESSION_PATH"))?
                .map(PathBuf::from)
                .unwrap_or_else(|| ctx.ironclaw_base_dir().join("session.json")),
        })
    }

    pub(in crate::config::llm) fn resolve_nearai_config(
        ctx: &EnvContext,
        settings: &Settings,
    ) -> Result<NearAiConfig, ConfigError> {
        let nearai_api_key =
            optional_env_from(ctx, EnvKey("NEARAI_API_KEY"))?.map(SecretString::from);
        Ok(NearAiConfig {
            model: Self::resolve_model(ctx, "NEARAI_MODEL", settings, "zai-org/GLM-latest")?,
            cheap_model: optional_env_from(ctx, EnvKey("NEARAI_CHEAP_MODEL"))?,
            base_url: optional_env_from(ctx, EnvKey("NEARAI_BASE_URL"))?.unwrap_or_else(|| {
                if nearai_api_key.is_some() {
                    "https://cloud-api.near.ai".to_string()
                } else {
                    "https://private.near.ai".to_string()
                }
            }),
            api_key: nearai_api_key,
            fallback_model: optional_env_from(ctx, EnvKey("NEARAI_FALLBACK_MODEL"))?,
            max_retries: parse_optional_env_from(ctx, EnvKey("NEARAI_MAX_RETRIES"), 3)?,
            circuit_breaker_threshold: optional_env_from(ctx, EnvKey("CIRCUIT_BREAKER_THRESHOLD"))?
                .map(|s| s.parse())
                .transpose()
                .map_err(|e| ConfigError::InvalidValue {
                    key: "CIRCUIT_BREAKER_THRESHOLD".to_string(),
                    message: format!("must be a positive integer: {e}"),
                })?,
            circuit_breaker_recovery_secs: parse_optional_env_from(
                ctx,
                EnvKey("CIRCUIT_BREAKER_RECOVERY_SECS"),
                30,
            )?,
            response_cache_enabled: parse_optional_env_from(
                ctx,
                EnvKey("RESPONSE_CACHE_ENABLED"),
                false,
            )?,
            response_cache_ttl_secs: parse_optional_env_from(
                ctx,
                EnvKey("RESPONSE_CACHE_TTL_SECS"),
                3600,
            )?,
            response_cache_max_entries: parse_optional_env_from(
                ctx,
                EnvKey("RESPONSE_CACHE_MAX_ENTRIES"),
                1000,
            )?,
            failover_cooldown_secs: parse_optional_env_from(
                ctx,
                EnvKey("LLM_FAILOVER_COOLDOWN_SECS"),
                300,
            )?,
            failover_cooldown_threshold: parse_optional_env_from(
                ctx,
                EnvKey("LLM_FAILOVER_THRESHOLD"),
                3,
            )?,
            smart_routing_cascade: parse_optional_env_from(
                ctx,
                EnvKey("SMART_ROUTING_CASCADE"),
                true,
            )?,
        })
    }

    pub(in crate::config::llm) fn resolve_bedrock_config(
        ctx: &EnvContext,
        settings: &Settings,
        is_bedrock: bool,
    ) -> Result<Option<BedrockConfig>, ConfigError> {
        if !is_bedrock {
            return Ok(None);
        }
        let region = resolve_bedrock_region(ctx, settings)?;
        let model = resolve_bedrock_model(ctx, settings)?;
        let cross_region = optional_env_from(ctx, EnvKey("BEDROCK_CROSS_REGION"))?
            .or_else(|| settings.bedrock_cross_region.clone());
        validate_bedrock_cross_region(&cross_region)?;
        let profile = optional_env_from(ctx, EnvKey("AWS_PROFILE"))?
            .or_else(|| settings.bedrock_profile.clone());
        Ok(Some(BedrockConfig {
            region,
            model,
            cross_region,
            profile,
        }))
    }

    /// Resolve a `RegistryProviderConfig` from the registry and env vars.
    pub(in crate::config::llm) fn resolve_registry_provider(
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
                backend: &spec.canonical_id,
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
