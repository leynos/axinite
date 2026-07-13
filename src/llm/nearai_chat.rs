//! NEAR AI provider implementation (Chat Completions API).
//!
//! This provider uses the OpenAI-compatible Chat Completions endpoint with
//! dual auth support:
//! - **API key auth**: When `NEARAI_API_KEY` is set, uses Bearer API key
//! - **Session token auth**: Otherwise, uses `SessionManager` for Bearer session token
//!   with automatic renewal on 401 errors

mod completion;
mod models;
mod pricing;
mod transport;
mod wire;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::Arc;

use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::llm::config::NearAiConfig;
use crate::llm::error::LlmError;
use crate::llm::session::SessionManager;

use pricing::fetch_pricing;

/// Information about an available model from NEAR AI API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Model identifier.
    #[serde(alias = "id", alias = "model")]
    pub name: String,
    /// Optional provider name.
    #[serde(default)]
    pub provider: Option<String>,
}

/// NEAR AI provider (Chat Completions API, dual auth).
pub struct NearAiChatProvider {
    client: Client,
    config: NearAiConfig,
    /// Session manager for session token auth (used when no API key is set).
    session: Arc<SessionManager>,
    active_model: std::sync::RwLock<String>,
    flatten_tool_messages: bool,
    /// Per-model pricing fetched from the NEAR AI `/v1/model/list` endpoint.
    /// Maps model ID → (input_cost_per_token, output_cost_per_token).
    pricing: Arc<std::sync::RwLock<HashMap<String, (Decimal, Decimal)>>>,
}

impl NearAiChatProvider {
    /// Create a new NEAR AI Chat Completions provider.
    ///
    /// Auth mode is determined by `config.api_key`:
    /// - If set, uses Bearer API key auth
    /// - If not set, uses session token auth via `SessionManager`
    ///
    /// By default this enables tool-message flattening for compatibility with
    /// providers that reject `role: "tool"` messages.
    pub fn new(config: NearAiConfig, session: Arc<SessionManager>) -> Result<Self, LlmError> {
        Self::new_with_options(config, session, true, 120)
    }

    /// Create a new provider with a custom request timeout.
    pub fn new_with_timeout(
        config: NearAiConfig,
        session: Arc<SessionManager>,
        request_timeout_secs: u64,
    ) -> Result<Self, LlmError> {
        Self::new_with_options(config, session, true, request_timeout_secs)
    }

    /// Create a chat completions provider with configurable tool-message flattening
    /// and request timeout.
    pub fn new_with_options(
        config: NearAiConfig,
        session: Arc<SessionManager>,
        flatten_tool_messages: bool,
        request_timeout_secs: u64,
    ) -> Result<Self, LlmError> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(request_timeout_secs))
            .build()
            .map_err(|e| LlmError::RequestFailed {
                provider: "nearai_chat".to_string(),
                reason: format!("Failed to build HTTP client: {}", e),
            })?;

        let active_model = std::sync::RwLock::new(config.model.clone());
        let pricing = Arc::new(std::sync::RwLock::new(HashMap::new()));

        let provider = Self {
            client,
            config,
            session,
            active_model,
            flatten_tool_messages,
            pricing,
        };

        // Fire-and-forget background pricing fetch — don't block startup.
        // Only spawns when a tokio runtime is active (skipped in sync tests).
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let client = provider.client.clone();
            let base_url = provider.config.base_url.clone();
            let api_key = provider.config.api_key.clone();
            let session = provider.session.clone();
            let pricing = provider.pricing.clone();

            handle.spawn(async move {
                match fetch_pricing(&client, &base_url, api_key.as_ref(), &session).await {
                    Ok(map) if !map.is_empty() => {
                        tracing::debug!("Loaded NEAR AI pricing for {} model(s)", map.len());
                        match pricing.write() {
                            Ok(mut guard) => *guard = map,
                            Err(poisoned) => *poisoned.into_inner() = map,
                        }
                    }
                    Ok(_) => {
                        tracing::debug!("NEAR AI pricing endpoint returned no pricing data");
                    }
                    Err(e) => {
                        tracing::debug!(
                            "Could not fetch NEAR AI pricing (will use fallback): {}",
                            e
                        );
                    }
                }
            });
        }

        Ok(provider)
    }

    #[cfg(test)]
    fn api_url(&self, path: &str) -> String {
        Self::api_url_for_base(&self.config.base_url, path)
    }

    fn api_url_for_base(base: &str, path: &str) -> String {
        let base = base.trim_end_matches('/');
        let path = path.trim_start_matches('/');

        if base.ends_with("/v1") {
            format!("{}/{}", base, path)
        } else {
            format!("{}/v1/{}", base, path)
        }
    }
}
