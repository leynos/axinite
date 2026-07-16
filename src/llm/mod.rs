//! LLM integration for the agent.
//!
//! Supports multiple backends:
//! - **NEAR AI** (default): Session token or API key auth via Chat Completions API
//! - **OpenAI**: Direct API access with your own key
//! - **Anthropic**: Direct API access with your own key
//! - **Ollama**: Local model inference
//! - **OpenAI-compatible**: Any endpoint that speaks the OpenAI API
//! - **AWS Bedrock**: Native Converse API via aws-sdk-bedrockruntime

mod anthropic_oauth;
#[cfg(feature = "bedrock")]
mod bedrock;
pub mod circuit_breaker;
pub mod config;
pub mod costs;
pub mod error;
pub mod failover;
mod nearai_chat;
pub mod oauth_helpers;
mod provider;
mod reasoning;
pub mod recording;
pub mod registry;
pub mod response_cache;
pub mod retry;
mod rig_adapter;

pub(crate) mod schema_normalize;
pub mod session;
pub mod smart_routing;

pub mod image_models;
pub mod reasoning_models;
#[cfg(test)]
pub(crate) mod test_fixtures;
pub mod vision_models;

pub use circuit_breaker::{CircuitBreakerConfig, CircuitBreakerProvider};
pub use config::{
    BedrockConfig, CacheRetention, LlmConfig, NearAiConfig, OAUTH_PLACEHOLDER,
    RegistryProviderConfig,
};
pub use error::LlmError;
pub use failover::{CooldownConfig, FailoverProvider};
pub use nearai_chat::{ModelInfo, NearAiChatProvider};
pub use provider::{
    ChatMessage, CompletionRequest, CompletionResponse, ContentPart, FinishReason, ImageUrl,
    LlmFuture, LlmProvider, ModelMetadata, NativeLlmProvider, Role, ToolCall,
    ToolCompletionRequest, ToolCompletionResponse, ToolDefinition, ToolResult,
};
pub use reasoning::{
    ActionPlan, Reasoning, ReasoningContext, RespondOutput, RespondResult, SILENT_REPLY_TOKEN,
    TOOL_INTENT_NUDGE, TokenUsage, ToolSelection, is_silent_reply, llm_signals_tool_intent,
};
pub use recording::RecordingLlm;
pub use registry::{ProviderDefinition, ProviderProtocol, ProviderRegistry};
pub use response_cache::{CachedProvider, ResponseCacheConfig};
pub use retry::{RetryConfig, RetryProvider};
pub use rig_adapter::RigAdapter;
pub use session::{SessionConfig, SessionManager, create_session_manager};
pub use smart_routing::{SmartRoutingConfig, SmartRoutingProvider, TaskComplexity};

mod chain;
mod factory;

pub use chain::build_provider_chain;
pub use factory::{
    create_cheap_llm_provider, create_llm_provider, create_llm_provider_with_config,
};

#[cfg(test)]
mod tests {
    //! Unit tests for LLM provider configuration and routing.

    use std::sync::Arc;

    use super::*;
    use crate::llm::config::NearAiConfig;

    fn test_nearai_config() -> NearAiConfig {
        NearAiConfig {
            model: "test-model".to_string(),
            cheap_model: None,
            base_url: "https://api.near.ai".to_string(),
            api_key: None,
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
        }
    }

    fn test_llm_config() -> LlmConfig {
        LlmConfig {
            backend: "nearai".to_string(),
            session: SessionConfig::default(),
            nearai: test_nearai_config(),
            provider: None,
            bedrock: None,
            request_timeout_secs: 120,
        }
    }

    #[test]
    fn test_create_cheap_llm_provider_returns_none_when_not_configured() {
        let config = test_llm_config();
        let session = Arc::new(SessionManager::new(SessionConfig::default()));

        let result = create_cheap_llm_provider(&config, session);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_create_cheap_llm_provider_creates_provider_when_configured() {
        let mut config = test_llm_config();
        config.nearai.cheap_model = Some("cheap-test-model".to_string());

        let session = Arc::new(SessionManager::new(SessionConfig::default()));
        let result = create_cheap_llm_provider(&config, session);

        assert!(result.is_ok());
        let provider = result.unwrap();
        assert!(provider.is_some());
        assert_eq!(provider.unwrap().model_name(), "cheap-test-model");
    }

    #[test]
    fn test_create_cheap_llm_provider_ignored_for_non_nearai_backend() {
        let mut config = test_llm_config();
        config.backend = "openai".to_string();
        config.nearai.cheap_model = Some("cheap-test-model".to_string());

        let session = Arc::new(SessionManager::new(SessionConfig::default()));
        let result = create_cheap_llm_provider(&config, session);

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }
}
