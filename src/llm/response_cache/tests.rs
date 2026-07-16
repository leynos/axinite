//! Unit tests for response caching and per-model cache key isolation,
//! with shared stub-provider and request helpers.

mod caching;
mod stats;

use std::sync::atomic::{AtomicU32, Ordering};

use rust_decimal::Decimal;

use crate::llm::error::LlmError;
use crate::llm::provider::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, ToolCompletionRequest,
    ToolCompletionResponse,
};

/// Minimal provider stub that supports `set_model()` — used to test
/// per-model cache key isolation.
struct SwitchableStub {
    call_count: AtomicU32,
    active_model: std::sync::RwLock<String>,
}

impl SwitchableStub {
    fn new() -> Self {
        Self {
            call_count: AtomicU32::new(0),
            active_model: std::sync::RwLock::new("stub-model".to_string()),
        }
    }
}

impl crate::llm::NativeLlmProvider for SwitchableStub {
    fn model_name(&self) -> &str {
        "stub-model"
    }

    fn active_model_name(&self) -> String {
        self.active_model.read().unwrap().clone()
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    fn set_model(&self, model: &str) -> Result<(), LlmError> {
        *self.active_model.write().unwrap() = model.to_string();
        Ok(())
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        Ok(CompletionResponse {
            content: "ok".into(),
            input_tokens: 1,
            output_tokens: 1,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        Ok(ToolCompletionResponse {
            content: Some("ok".into()),
            tool_calls: vec![],
            input_tokens: 1,
            output_tokens: 1,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }
}

fn simple_request() -> CompletionRequest {
    CompletionRequest {
        messages: vec![ChatMessage::user("hello")],
        model: None,
        max_tokens: None,
        temperature: None,
        stop_sequences: None,
        metadata: Default::default(),
    }
}

fn different_request() -> CompletionRequest {
    CompletionRequest {
        messages: vec![ChatMessage::user("goodbye")],
        model: None,
        max_tokens: None,
        temperature: None,
        stop_sequences: None,
        metadata: Default::default(),
    }
}
