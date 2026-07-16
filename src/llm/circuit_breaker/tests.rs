//! Unit tests for circuit breaker state transitions and recovery,
//! with shared request/config helpers.

mod chaos;
mod transitions;

use std::time::Duration;

use crate::llm::provider::{CompletionRequest, ToolCompletionRequest};

use super::CircuitBreakerConfig;

fn make_request() -> CompletionRequest {
    CompletionRequest::new(vec![crate::llm::ChatMessage::user("hello")])
}

fn make_tool_request() -> ToolCompletionRequest {
    ToolCompletionRequest::new(vec![crate::llm::ChatMessage::user("hello")], vec![])
}

fn fast_config(threshold: u32) -> CircuitBreakerConfig {
    CircuitBreakerConfig {
        failure_threshold: threshold,
        recovery_timeout: Duration::from_millis(50),
        half_open_successes_needed: 1,
    }
}
