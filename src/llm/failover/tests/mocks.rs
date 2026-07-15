//! Mock LLM providers shared by the failover test modules.

use std::sync::atomic::AtomicU32;
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use super::super::*;

use crate::llm::provider::{CompletionResponse, FinishReason, ToolCompletionResponse};

/// A mock LLM provider that returns a predetermined result.
pub(super) struct MockProvider {
    name: String,
    active_model: RwLock<String>,
    input_cost: Decimal,
    output_cost: Decimal,
    complete_result: Mutex<Option<Result<CompletionResponse, LlmError>>>,
    tool_complete_result: Mutex<Option<Result<ToolCompletionResponse, LlmError>>>,
}

impl MockProvider {
    pub(super) fn succeeding(name: &str, content: &str) -> Self {
        Self {
            name: name.to_string(),
            active_model: RwLock::new(name.to_string()),
            input_cost: Decimal::ZERO,
            output_cost: Decimal::ZERO,
            complete_result: Mutex::new(Some(Ok(CompletionResponse {
                content: content.to_string(),
                input_tokens: 10,
                output_tokens: 5,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            }))),
            tool_complete_result: Mutex::new(Some(Ok(ToolCompletionResponse {
                content: Some(content.to_string()),
                tool_calls: vec![],
                input_tokens: 10,
                output_tokens: 5,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            }))),
        }
    }

    pub(super) fn succeeding_with_cost(
        name: &str,
        content: &str,
        input_cost: Decimal,
        output_cost: Decimal,
    ) -> Self {
        Self {
            input_cost,
            output_cost,
            ..Self::succeeding(name, content)
        }
    }

    pub(super) fn failing_retryable(name: &str) -> Self {
        Self {
            name: name.to_string(),
            active_model: RwLock::new(name.to_string()),
            input_cost: Decimal::ZERO,
            output_cost: Decimal::ZERO,
            complete_result: Mutex::new(Some(Err(LlmError::RequestFailed {
                provider: name.to_string(),
                reason: "server error".to_string(),
            }))),
            tool_complete_result: Mutex::new(Some(Err(LlmError::RequestFailed {
                provider: name.to_string(),
                reason: "server error".to_string(),
            }))),
        }
    }

    pub(super) fn failing_non_retryable(name: &str) -> Self {
        Self {
            name: name.to_string(),
            active_model: RwLock::new(name.to_string()),
            input_cost: Decimal::ZERO,
            output_cost: Decimal::ZERO,
            complete_result: Mutex::new(Some(Err(LlmError::AuthFailed {
                provider: name.to_string(),
            }))),
            tool_complete_result: Mutex::new(Some(Err(LlmError::AuthFailed {
                provider: name.to_string(),
            }))),
        }
    }

    pub(super) fn failing_rate_limited(name: &str) -> Self {
        Self {
            name: name.to_string(),
            active_model: RwLock::new(name.to_string()),
            input_cost: Decimal::ZERO,
            output_cost: Decimal::ZERO,
            complete_result: Mutex::new(Some(Err(LlmError::RateLimited {
                provider: name.to_string(),
                retry_after: Some(Duration::from_secs(30)),
            }))),
            tool_complete_result: Mutex::new(Some(Err(LlmError::RateLimited {
                provider: name.to_string(),
                retry_after: Some(Duration::from_secs(30)),
            }))),
        }
    }
}

impl crate::llm::NativeLlmProvider for MockProvider {
    fn model_name(&self) -> &str {
        &self.name
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (self.input_cost, self.output_cost)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.complete_result
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| {
                Err(LlmError::InvalidResponse {
                    provider: self.name.clone(),
                    reason: "MockProvider::complete called more than once".to_string(),
                })
            })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        self.tool_complete_result
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| {
                Err(LlmError::InvalidResponse {
                    provider: self.name.clone(),
                    reason: "MockProvider::complete_with_tools called more than once".to_string(),
                })
            })
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        Ok(vec![self.name.clone()])
    }

    fn active_model_name(&self) -> String {
        self.active_model.read().unwrap().clone()
    }

    fn set_model(&self, model: &str) -> Result<(), LlmError> {
        *self.active_model.write().unwrap() = model.to_string();
        Ok(())
    }
}

pub(super) fn make_request() -> CompletionRequest {
    CompletionRequest::new(vec![crate::llm::ChatMessage::user("hello")])
}

pub(super) fn make_tool_request() -> ToolCompletionRequest {
    ToolCompletionRequest::new(vec![crate::llm::ChatMessage::user("hello")], vec![])
}

// --- MultiCallMockProvider for cooldown tests ---
//
// Unlike `MockProvider` which uses `.take()` (single-use), this mock
// tracks a call counter and returns errors for the first N calls,
// then succeeds.

pub(super) struct MultiCallMockProvider {
    name: String,
    /// How many calls should fail before succeeding. 0 = always succeed.
    fail_count: u32,
    /// Atomically tracks how many times `complete` has been called.
    calls: AtomicU32,
    /// If true, failures are non-retryable (AuthFailed).
    non_retryable: bool,
}

impl MultiCallMockProvider {
    /// Always succeeds.
    pub(super) fn always_ok(name: &str) -> Self {
        Self {
            name: name.to_string(),
            fail_count: 0,
            calls: AtomicU32::new(0),
            non_retryable: false,
        }
    }

    /// Fails with retryable error for the first `n` calls, then succeeds.
    pub(super) fn fail_then_ok(name: &str, n: u32) -> Self {
        Self {
            name: name.to_string(),
            fail_count: n,
            calls: AtomicU32::new(0),
            non_retryable: false,
        }
    }

    /// Always fails with retryable error.
    pub(super) fn always_fail(name: &str) -> Self {
        Self {
            name: name.to_string(),
            fail_count: u32::MAX,
            calls: AtomicU32::new(0),
            non_retryable: false,
        }
    }

    /// Always fails with non-retryable error.
    pub(super) fn always_fail_non_retryable(name: &str) -> Self {
        Self {
            name: name.to_string(),
            fail_count: u32::MAX,
            calls: AtomicU32::new(0),
            non_retryable: true,
        }
    }

    pub(super) fn call_count(&self) -> u32 {
        self.calls.load(Ordering::Relaxed)
    }
}

impl crate::llm::NativeLlmProvider for MultiCallMockProvider {
    fn model_name(&self) -> &str {
        &self.name
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let n = self.calls.fetch_add(1, Ordering::Relaxed);
        if n < self.fail_count {
            if self.non_retryable {
                return Err(LlmError::AuthFailed {
                    provider: self.name.clone(),
                });
            }
            return Err(LlmError::RequestFailed {
                provider: self.name.clone(),
                reason: format!("call {} failed", n),
            });
        }
        Ok(CompletionResponse {
            content: format!("{} ok", self.name),
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
        let n = self.calls.fetch_add(1, Ordering::Relaxed);
        if n < self.fail_count {
            if self.non_retryable {
                return Err(LlmError::AuthFailed {
                    provider: self.name.clone(),
                });
            }
            return Err(LlmError::RequestFailed {
                provider: self.name.clone(),
                reason: format!("call {} failed", n),
            });
        }
        Ok(ToolCompletionResponse {
            content: Some(format!("{} ok", self.name)),
            tool_calls: vec![],
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        Ok(vec![self.name.clone()])
    }
}
