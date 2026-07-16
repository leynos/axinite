//! Unit tests for the NearAI chat provider configuration and
//! requests.

mod messages;
mod pricing_costs;
mod requests;
mod responses;
mod urls_auth;

use std::sync::Arc;

use crate::llm::config::NearAiConfig;
use crate::llm::session::{SessionConfig, SessionManager};

pub(super) fn test_nearai_config(base_url: &str) -> NearAiConfig {
    NearAiConfig {
        model: "test-model".to_string(),
        base_url: base_url.to_string(),
        api_key: Some(secrecy::SecretString::from("test-key".to_string())),
        cheap_model: None,
        fallback_model: None,
        max_retries: 0,
        circuit_breaker_threshold: None,
        circuit_breaker_recovery_secs: 30,
        response_cache_enabled: false,
        response_cache_ttl_secs: 3600,
        response_cache_max_entries: 1000,
        failover_cooldown_secs: 300,
        failover_cooldown_threshold: 3,
        smart_routing_cascade: true,
    }
}

pub(super) fn test_session() -> Arc<SessionManager> {
    Arc::new(SessionManager::new(SessionConfig::default()))
}
