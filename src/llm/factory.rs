//! Provider factory functions for the LLM subsystem.
//!
//! Constructs individual `LlmProvider` instances from configuration:
//! NEAR AI, registry-resolved providers (OpenAI-compatible, Anthropic,
//! Ollama), AWS Bedrock, and the optional cheap model for lightweight tasks.

use std::sync::Arc;

use rig::client::CompletionClient;
use rig::providers::anthropic;
use secrecy::ExposeSecret;

use super::anthropic_oauth;
#[cfg(feature = "bedrock")]
use super::bedrock;
use super::config::{CacheRetention, LlmConfig, NearAiConfig, RegistryProviderConfig};
use super::error::LlmError;
use super::nearai_chat::NearAiChatProvider;
use super::provider::LlmProvider;
use super::registry::ProviderProtocol;
use super::rig_adapter::RigAdapter;
use super::session::SessionManager;

/// Whether the configured backend name selects the NEAR AI provider.
fn is_nearai_backend(backend: &str) -> bool {
    matches!(backend, "nearai" | "near_ai" | "near")
}

/// Create an LLM provider based on configuration.
///
/// - NearAI backend: Uses session manager for authentication
/// - Registry providers: Looked up by protocol and constructed generically
pub async fn create_llm_provider(
    config: &LlmConfig,
    session: Arc<SessionManager>,
) -> Result<Arc<dyn LlmProvider>, LlmError> {
    let timeout = config.request_timeout_secs;

    if is_nearai_backend(&config.backend) {
        return create_llm_provider_with_config(&config.nearai, session, timeout);
    }

    // Bedrock uses a native AWS SDK, not the rig-core registry
    if config.backend == "bedrock" {
        #[cfg(feature = "bedrock")]
        {
            return create_bedrock_provider(config).await;
        }
        #[cfg(not(feature = "bedrock"))]
        {
            return Err(LlmError::RequestFailed {
                provider: "bedrock".to_string(),
                reason: "Bedrock support not compiled. Rebuild with --features bedrock".to_string(),
            });
        }
    }

    let reg_config = config
        .provider
        .as_ref()
        .ok_or_else(|| LlmError::AuthFailed {
            provider: config.backend.clone(),
        })?;

    create_registry_provider(reg_config)
}

/// Create an LLM provider from a `NearAiConfig` directly.
///
/// This is useful when constructing additional providers for failover,
/// where only the model name differs from the primary config.
pub fn create_llm_provider_with_config(
    config: &NearAiConfig,
    session: Arc<SessionManager>,
    request_timeout_secs: u64,
) -> Result<Arc<dyn LlmProvider>, LlmError> {
    let auth_mode = if config.api_key.is_some() {
        "API key"
    } else {
        "session token"
    };
    tracing::debug!(
        model = %config.model,
        base_url = %config.base_url,
        auth = auth_mode,
        timeout_secs = request_timeout_secs,
        "Using NEAR AI (Chat Completions API)"
    );
    Ok(Arc::new(NearAiChatProvider::new_with_timeout(
        config.clone(),
        session,
        request_timeout_secs,
    )?))
}

/// Create a provider from a registry-resolved config.
///
/// Dispatches on `RegistryProviderConfig::protocol` to build the appropriate
/// rig-core client. This single function replaces what used to be 5 separate
/// `create_*_provider` functions.
fn create_registry_provider(
    config: &RegistryProviderConfig,
) -> Result<Arc<dyn LlmProvider>, LlmError> {
    match config.protocol {
        ProviderProtocol::OpenAiCompletions => create_openai_compat_from_registry(config),
        ProviderProtocol::Anthropic => create_anthropic_from_registry(config),
        ProviderProtocol::Ollama => create_ollama_from_registry(config),
    }
}

#[cfg(feature = "bedrock")]
async fn create_bedrock_provider(config: &LlmConfig) -> Result<Arc<dyn LlmProvider>, LlmError> {
    let br = config
        .bedrock
        .as_ref()
        .ok_or_else(|| LlmError::AuthFailed {
            provider: "bedrock".to_string(),
        })?;

    let provider = bedrock::BedrockProvider::new(br).await?;
    tracing::debug!(
        "Using AWS Bedrock (Converse API, region: {}, model: {})",
        br.region,
        crate::llm::NativeLlmProvider::active_model_name(&provider),
    );
    Ok(Arc::new(provider))
}

fn create_openai_compat_from_registry(
    config: &RegistryProviderConfig,
) -> Result<Arc<dyn LlmProvider>, LlmError> {
    use rig::providers::openai;

    let extra_headers = build_extra_headers(&config.extra_headers);

    let api_key = config
        .api_key
        .as_ref()
        .map(|k| k.expose_secret().to_string())
        .unwrap_or_else(|| {
            tracing::warn!(
                provider = %config.provider_id,
                "No API key configured for {}. Requests will likely fail with 401. \
                 Check your .env or secrets store.",
                config.provider_id,
            );
            "no-key".to_string()
        });

    let mut builder = openai::Client::builder().api_key(&api_key);
    if !config.base_url.is_empty() {
        builder = builder.base_url(&config.base_url);
    }
    if !extra_headers.is_empty() {
        builder = builder.http_headers(extra_headers);
    }

    let client: openai::Client = builder.build().map_err(|e| LlmError::RequestFailed {
        provider: config.provider_id.clone(),
        reason: format!("Failed to create OpenAI-compatible client: {e}"),
    })?;

    // Use CompletionsClient (Chat Completions API) instead of the default
    // Client (Responses API). The Responses API path in rig-core handles
    // tool results differently, which breaks Axinite's tool call flow.
    let client = client.completions_api();
    let model = client.completion_model(&config.model);

    tracing::debug!(
        provider = %config.provider_id,
        model = %config.model,
        base_url = %config.base_url,
        "Using OpenAI-compatible provider"
    );

    let adapter = RigAdapter::new(model, &config.model)
        .with_unsupported_params(config.unsupported_params.clone());
    Ok(Arc::new(adapter))
}

/// Build the extra HTTP header map from configured name/value pairs.
///
/// Invalid names or values are logged and skipped so one bad entry does not
/// prevent the provider from being created.
fn build_extra_headers(configured: &[(String, String)]) -> reqwest::header::HeaderMap {
    let mut extra_headers = reqwest::header::HeaderMap::new();
    for (key, value) in configured {
        let Some(name) = parse_header_name(key) else {
            continue;
        };
        let Some(value) = parse_header_value(key, value) else {
            continue;
        };
        extra_headers.insert(name, value);
    }
    extra_headers
}

/// Parse a configured header name, logging and returning `None` when invalid.
fn parse_header_name(key: &str) -> Option<reqwest::header::HeaderName> {
    match reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
        Ok(name) => Some(name),
        Err(error) => {
            tracing::warn!(header = %key, error = %error, "Skipping extra header: invalid name");
            None
        }
    }
}

/// Parse a configured header value, logging and returning `None` when invalid.
fn parse_header_value(key: &str, value: &str) -> Option<reqwest::header::HeaderValue> {
    match reqwest::header::HeaderValue::from_str(value) {
        Ok(value) => Some(value),
        Err(error) => {
            tracing::warn!(header = %key, error = %error, "Skipping extra header: invalid value");
            None
        }
    }
}

/// Whether the registry config should authenticate via Anthropic OAuth: an
/// OAuth token is present and no real API key was provided (either absent or
/// the placeholder value). When both are set, the API key takes priority
/// (standard x-api-key auth via rig-core).
fn should_use_anthropic_oauth(config: &RegistryProviderConfig) -> bool {
    let api_key_is_placeholder = config
        .api_key
        .as_ref()
        .is_some_and(|k| k.expose_secret() == crate::llm::config::OAUTH_PLACEHOLDER);
    let no_real_api_key = config.api_key.is_none() || api_key_is_placeholder;
    config.oauth_token.is_some() && no_real_api_key
}

fn create_anthropic_from_registry(
    config: &RegistryProviderConfig,
) -> Result<Arc<dyn LlmProvider>, LlmError> {
    if should_use_anthropic_oauth(config) {
        return create_anthropic_oauth_provider(config);
    }

    let api_key = config
        .api_key
        .as_ref()
        .map(|k| k.expose_secret().to_string())
        .ok_or_else(|| LlmError::AuthFailed {
            provider: config.provider_id.clone(),
        })?;

    let client = create_anthropic_client(&api_key, config)?;
    let cache_retention = config.cache_retention;
    let model = client.completion_model(&config.model);

    log_cache_retention(config, cache_retention);

    tracing::debug!(
        provider = %config.provider_id,
        model = %config.model,
        base_url = describe_base_url(config),
        "Using Anthropic provider"
    );

    Ok(Arc::new(
        RigAdapter::new(model, &config.model)
            .with_cache_retention(cache_retention)
            .with_unsupported_params(config.unsupported_params.clone()),
    ))
}

/// Describe the configured base URL for diagnostic output.
fn describe_base_url(config: &RegistryProviderConfig) -> &str {
    if config.base_url.is_empty() {
        "default"
    } else {
        &config.base_url
    }
}

/// Build the Anthropic provider that authenticates with an OAuth token.
fn create_anthropic_oauth_provider(
    config: &RegistryProviderConfig,
) -> Result<Arc<dyn LlmProvider>, LlmError> {
    tracing::debug!(
        provider = %config.provider_id,
        model = %config.model,
        base_url = describe_base_url(config),
        "Using Anthropic OAuth API"
    );

    let provider = anthropic_oauth::AnthropicOAuthProvider::new(config)?;
    Ok(Arc::new(provider))
}

/// Build the Anthropic client for API-key authentication.
fn create_anthropic_client(
    api_key: &str,
    config: &RegistryProviderConfig,
) -> Result<anthropic::Client, LlmError> {
    if config.base_url.is_empty() {
        anthropic::Client::new(api_key)
    } else {
        anthropic::Client::builder()
            .api_key(api_key)
            .base_url(&config.base_url)
            .build()
    }
    .map_err(|e| LlmError::RequestFailed {
        provider: config.provider_id.clone(),
        reason: format!("Failed to create Anthropic client: {e}"),
    })
}

/// Log the prompt-caching decision when caching is enabled.
fn log_cache_retention(config: &RegistryProviderConfig, retention: CacheRetention) {
    if retention != CacheRetention::None {
        tracing::debug!(
            model = %config.model,
            retention = %retention,
            "Anthropic automatic prompt caching enabled"
        );
    }
}

fn create_ollama_from_registry(
    config: &RegistryProviderConfig,
) -> Result<Arc<dyn LlmProvider>, LlmError> {
    use rig::client::Nothing;
    use rig::providers::ollama;

    let client: ollama::Client = ollama::Client::builder()
        .base_url(&config.base_url)
        .api_key(Nothing)
        .build()
        .map_err(|e| LlmError::RequestFailed {
            provider: config.provider_id.clone(),
            reason: format!("Failed to create Ollama client: {e}"),
        })?;

    let model = client.completion_model(&config.model);

    tracing::debug!(
        provider = %config.provider_id,
        model = %config.model,
        base_url = %config.base_url,
        "Using Ollama provider"
    );

    let adapter = RigAdapter::new(model, &config.model)
        .with_unsupported_params(config.unsupported_params.clone());
    Ok(Arc::new(adapter))
}

/// Create a cheap/fast LLM provider for lightweight tasks (heartbeat, routing, evaluation).
///
/// Uses `NEARAI_CHEAP_MODEL` if set, otherwise falls back to the main provider.
/// Currently only supports NEAR AI backend.
pub fn create_cheap_llm_provider(
    config: &LlmConfig,
    session: Arc<SessionManager>,
) -> Result<Option<Arc<dyn LlmProvider>>, LlmError> {
    let Some(ref cheap_model) = config.nearai.cheap_model else {
        return Ok(None);
    };

    if config.backend != "nearai" {
        tracing::warn!(
            "NEARAI_CHEAP_MODEL is set but LLM_BACKEND is '{}', not nearai. \
             Cheap model setting will be ignored.",
            config.backend
        );
        return Ok(None);
    }

    let mut cheap_config = config.nearai.clone();
    cheap_config.model = cheap_model.clone();

    Ok(Some(Arc::new(NearAiChatProvider::new(
        cheap_config,
        session,
    )?)))
}
