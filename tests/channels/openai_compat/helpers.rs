//! Mock LLM providers and server helpers shared by the OpenAI-compatible
//! API integration tests.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rust_decimal::Decimal;

use ironclaw::channels::web::server::{GatewayState, start_server};
use ironclaw::channels::web::sse::SseManager;
use ironclaw::channels::web::ws::WsConnectionTracker;
use ironclaw::error::LlmError;
use ironclaw::llm::{
    CompletionRequest, CompletionResponse, FinishReason, LlmProvider, ToolCompletionRequest,
    ToolCompletionResponse,
};

pub(super) const AUTH_TOKEN: &str = "test-openai-token";

// ---------------------------------------------------------------------------
// Mock LLM provider
// ---------------------------------------------------------------------------

#[derive(Default)]
pub(super) struct MockLlmState {
    pub(super) completion_models: tokio::sync::Mutex<Vec<Option<String>>>,
    pub(super) tool_completion_models: tokio::sync::Mutex<Vec<Option<String>>>,
}

struct MockLlmProvider {
    state: Arc<MockLlmState>,
}

impl MockLlmProvider {
    fn new(state: Arc<MockLlmState>) -> Self {
        Self { state }
    }
}

impl ironclaw::llm::NativeLlmProvider for MockLlmProvider {
    fn model_name(&self) -> &str {
        "mock-model-v1"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.state
            .completion_models
            .lock()
            .await
            .push(req.model.clone());

        // Echo the last user message back
        let user_msg = req
            .messages
            .iter()
            .rev()
            .find(|m| m.role == ironclaw::llm::Role::User)
            .map(|m| m.content.clone())
            .unwrap_or_else(|| "no user message".to_string());

        Ok(CompletionResponse {
            content: format!("Mock response to: {}", user_msg),
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        req: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        self.state
            .tool_completion_models
            .lock()
            .await
            .push(req.model.clone());

        // If tools are provided, return a tool call
        if let Some(tool) = req.tools.first() {
            Ok(ToolCompletionResponse {
                content: None,
                tool_calls: vec![ironclaw::llm::ToolCall {
                    id: "call_mock_001".to_string(),
                    name: tool.name.clone(),
                    arguments: serde_json::json!({"test": true}),
                }],
                input_tokens: 15,
                output_tokens: 8,
                finish_reason: FinishReason::ToolUse,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        } else {
            Ok(ToolCompletionResponse {
                content: Some("No tools available".to_string()),
                tool_calls: vec![],
                input_tokens: 10,
                output_tokens: 4,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        Ok(vec![
            "mock-model-v1".to_string(),
            "mock-model-v2".to_string(),
        ])
    }
}

pub(super) struct FixedModelProvider {
    model: &'static str,
}

impl FixedModelProvider {
    pub(super) fn new(model: &'static str) -> Self {
        Self { model }
    }
}

impl ironclaw::llm::NativeLlmProvider for FixedModelProvider {
    fn model_name(&self) -> &str {
        self.model
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        Ok(CompletionResponse {
            content: "fixed response".to_string(),
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        _req: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        Ok(ToolCompletionResponse {
            content: Some("fixed response".to_string()),
            tool_calls: vec![],
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    fn effective_model_name(&self, _requested_model: Option<&str>) -> String {
        self.model.to_string()
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

pub(super) async fn start_test_server() -> (SocketAddr, Arc<GatewayState>, Arc<MockLlmState>) {
    let mock_state = Arc::new(MockLlmState::default());

    let llm_provider: Arc<dyn LlmProvider> = Arc::new(MockLlmProvider::new(mock_state.clone()));
    let (bound_addr, state) = start_test_server_with_provider(llm_provider).await;

    (bound_addr, state, mock_state)
}

pub(super) async fn start_test_server_with_provider(
    llm_provider: Arc<dyn LlmProvider>,
) -> (SocketAddr, Arc<GatewayState>) {
    let state = Arc::new(GatewayState {
        msg_tx: tokio::sync::RwLock::new(None),
        sse: SseManager::new(),
        workspace: None,
        session_manager: None,
        log_broadcaster: None,
        log_level_handle: None,
        extension_manager: None,
        tool_registry: None,
        store: None,
        job_manager: None,
        prompt_queue: None,
        scheduler: None,
        user_id: "test-user".to_string(),
        shutdown_tx: tokio::sync::RwLock::new(None),
        ws_tracker: Some(Arc::new(WsConnectionTracker::new())),
        llm_provider: Some(llm_provider),
        skill_registry: None,
        skill_catalog: None,
        chat_rate_limiter: ironclaw::channels::web::server::RateLimiter::new(30, 60),
        oauth_rate_limiter: ironclaw::channels::web::server::RateLimiter::new(10, 60),
        registry_entries: Vec::new(),
        cost_guard: None,
        routine_engine: Arc::new(tokio::sync::RwLock::new(None)),
        startup_time: std::time::Instant::now(),
    });

    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let bound_addr = start_server(addr, state.clone(), AUTH_TOKEN.to_string())
        .await
        .expect("Failed to start test server");

    (bound_addr, state)
}

pub(super) fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap()
}
