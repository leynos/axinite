//! Mock LLM providers and request helpers for the provider chaos tests.
//!
//! - `FlakeyProvider` -- Fails N times, then succeeds
//! - `HangingProvider` -- Hangs forever (tests caller-side timeout)
//! - `GarbageProvider` -- Returns valid response structure with garbage content
//! - `ReliableProvider` -- Always succeeds (fallback target)

use std::sync::atomic::{AtomicU32, Ordering};

use rust_decimal::Decimal;

use ironclaw::error::LlmError;
use ironclaw::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, ToolCompletionRequest,
    ToolCompletionResponse,
};

// ---------------------------------------------------------------------------
// Mock providers
// ---------------------------------------------------------------------------

/// Provider that fails N times then succeeds.
///
/// Thread-safe: uses atomic counter so it works correctly across retries
/// and concurrent access.
pub(super) struct FlakeyProvider {
    failures_remaining: AtomicU32,
    success_response: String,
    name: String,
    call_count: AtomicU32,
}

impl FlakeyProvider {
    pub(super) fn new(failures: u32, response: impl Into<String>) -> Self {
        Self {
            failures_remaining: AtomicU32::new(failures),
            success_response: response.into(),
            name: "flakey".to_string(),
            call_count: AtomicU32::new(0),
        }
    }

    pub(super) fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    pub(super) fn calls(&self) -> u32 {
        self.call_count.load(Ordering::Relaxed)
    }
}

impl ironclaw::llm::NativeLlmProvider for FlakeyProvider {
    fn model_name(&self) -> &str {
        &self.name
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        let prev = self.failures_remaining.load(Ordering::Relaxed);
        if prev > 0 {
            // Attempt to decrement; if another thread decremented first, that's fine.
            let _ = self.failures_remaining.compare_exchange(
                prev,
                prev - 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
            return Err(LlmError::RequestFailed {
                provider: self.name.clone(),
                reason: format!("transient failure ({} remaining)", prev - 1),
            });
        }
        Ok(CompletionResponse {
            content: self.success_response.clone(),
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        let prev = self.failures_remaining.load(Ordering::Relaxed);
        if prev > 0 {
            let _ = self.failures_remaining.compare_exchange(
                prev,
                prev - 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
            return Err(LlmError::RequestFailed {
                provider: self.name.clone(),
                reason: format!("transient failure ({} remaining)", prev - 1),
            });
        }
        Ok(ToolCompletionResponse {
            content: Some(self.success_response.clone()),
            tool_calls: vec![],
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }
}

/// Provider that hangs forever (tests timeout handling at the caller).
pub(super) struct HangingProvider {
    name: String,
}

impl HangingProvider {
    pub(super) fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl ironclaw::llm::NativeLlmProvider for HangingProvider {
    fn model_name(&self) -> &str {
        &self.name
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        // Hang forever -- callers must use tokio::time::timeout.
        std::future::pending().await
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        std::future::pending().await
    }
}

/// Provider that returns valid response structures but with garbage content.
///
/// This tests that the system handles "technically valid but semantically
/// nonsensical" responses gracefully.
pub(super) struct GarbageProvider {
    name: String,
    call_count: AtomicU32,
}

impl GarbageProvider {
    pub(super) fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            call_count: AtomicU32::new(0),
        }
    }

    pub(super) fn calls(&self) -> u32 {
        self.call_count.load(Ordering::Relaxed)
    }
}

impl ironclaw::llm::NativeLlmProvider for GarbageProvider {
    fn model_name(&self) -> &str {
        &self.name
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        Ok(CompletionResponse {
            content: "\x00\x01\x02\x7f garbage \u{FFFD} response".to_string(),
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: FinishReason::Unknown,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        Ok(ToolCompletionResponse {
            content: Some(String::new()), // empty content
            tool_calls: vec![],
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: FinishReason::Unknown,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }
}

/// Simple always-ok provider for use as a reliable fallback in tests.
pub(super) struct ReliableProvider {
    name: String,
    response: String,
    call_count: AtomicU32,
}

impl ReliableProvider {
    pub(super) fn new(name: impl Into<String>, response: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            response: response.into(),
            call_count: AtomicU32::new(0),
        }
    }

    pub(super) fn calls(&self) -> u32 {
        self.call_count.load(Ordering::Relaxed)
    }
}

impl ironclaw::llm::NativeLlmProvider for ReliableProvider {
    fn model_name(&self) -> &str {
        &self.name
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        Ok(CompletionResponse {
            content: self.response.clone(),
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        self.call_count.fetch_add(1, Ordering::Relaxed);
        Ok(ToolCompletionResponse {
            content: Some(self.response.clone()),
            tool_calls: vec![],
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(super) fn make_request() -> CompletionRequest {
    CompletionRequest::new(vec![ChatMessage::user("hello")])
}

pub(super) fn make_tool_request() -> ToolCompletionRequest {
    ToolCompletionRequest::new(vec![ChatMessage::user("hello")], vec![])
}
