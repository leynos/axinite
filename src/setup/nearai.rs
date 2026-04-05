//! NEAR AI setup helpers.
//!
//! This module isolates provider-specific model discovery and config
//! construction used by the setup wizard during NEAR AI onboarding.

use std::sync::Arc;

use secrecy::{ExposeSecret, SecretString};

use super::print_info;
use crate::llm::{SessionConfig, SessionManager, create_llm_provider};

pub(super) async fn fetch_nearai_models(
    session_manager: Option<&Arc<SessionManager>>,
) -> Vec<String> {
    let session = match session_manager {
        Some(session) => Arc::clone(session),
        None => return vec![],
    };

    let config = build_nearai_model_fetch_config(session.get_api_key().await);

    match create_llm_provider(&config, session).await {
        Ok(provider) => match provider.list_models().await {
            Ok(models) => models,
            Err(error) => {
                print_info(&format!(
                    "Could not fetch models: {}. Using defaults.",
                    error
                ));
                vec![]
            }
        },
        Err(error) => {
            print_info(&format!(
                "Could not initialize provider: {}. Using defaults.",
                error
            ));
            vec![]
        }
    }
}

/// Build the `LlmConfig` used by `fetch_nearai_models` to list available models.
///
/// Uses the current in-memory NEAR AI Cloud API key, when present, so users
/// who authenticated via option 4 do not get re-prompted during model
/// selection.
pub(super) fn build_nearai_model_fetch_config(
    api_key: Option<SecretString>,
) -> crate::config::LlmConfig {
    let api_key = api_key.filter(|key| !key.expose_secret().trim().is_empty());

    // Match the same base_url logic as LlmConfig::resolve(): use cloud-api
    // when an API key is present, private.near.ai for session-token auth.
    let default_base = if api_key.is_some() {
        "https://cloud-api.near.ai"
    } else {
        "https://private.near.ai"
    };
    let base_url = std::env::var("NEARAI_BASE_URL").unwrap_or_else(|_| default_base.to_string());
    let auth_base_url =
        std::env::var("NEARAI_AUTH_URL").unwrap_or_else(|_| "https://private.near.ai".to_string());

    crate::config::LlmConfig {
        backend: "nearai".to_string(),
        session: SessionConfig {
            auth_base_url,
            session_path: crate::config::llm::default_session_path(),
        },
        nearai: crate::config::NearAiConfig {
            model: "dummy".to_string(),
            cheap_model: None,
            base_url,
            api_key,
            fallback_model: None,
            max_retries: 3,
            circuit_breaker_threshold: None,
            circuit_breaker_recovery_secs: 30,
            response_cache_enabled: false,
            response_cache_ttl_secs: 3600,
            response_cache_max_entries: 1000,
            failover_cooldown_secs: 300,
            failover_cooldown_threshold: 3,
            smart_routing_cascade: true,
        },
        provider: None,
        bedrock: None,
        request_timeout_secs: 120,
    }
}
