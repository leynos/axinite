//! Shared parameter objects and assertion helpers for the trace LLM tests.

use ironclaw::llm::{ChatMessage, Role, ToolCall};

#[derive(Copy, Clone, Debug)]
pub(crate) struct LlmCounterSnapshot {
    pub(crate) calls: usize,
    pub(crate) input_tokens: u32,
    pub(crate) output_tokens: u32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct LlmCounterMinima {
    pub(crate) calls: usize,
    pub(crate) input_tokens: u32,
    pub(crate) output_tokens: u32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ExpectedToolCall<'a> {
    pub(crate) name: &'a str,
    pub(crate) id: &'a str,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct CapturedRequestsExpectation<'a> {
    pub(crate) batches: usize,
    pub(crate) last_user_contains: &'a str,
    pub(crate) min_msgs_per_batch: usize,
}

#[track_caller]
pub(crate) fn assert_msg(role: Role, msg: &ChatMessage, contains: &str) {
    assert_eq!(msg.role, role);
    assert!(
        msg.content.contains(contains),
        "expected message content {:?} to contain {:?}",
        msg.content,
        contains
    );
}

#[track_caller]
pub(crate) fn assert_captured_requests_shape(
    captured: &[Vec<ChatMessage>],
    expected: CapturedRequestsExpectation<'_>,
) {
    assert_eq!(captured.len(), expected.batches);
    assert!(
        captured
            .iter()
            .all(|batch| batch.len() >= expected.min_msgs_per_batch),
        "expected every captured batch to contain at least {} messages",
        expected.min_msgs_per_batch
    );
    let last_batch = captured
        .last()
        .expect("captured requests should contain at least one batch");
    let last_message = last_batch
        .last()
        .expect("captured request batch should contain at least one message");
    assert_msg(Role::User, last_message, expected.last_user_contains);
}

#[track_caller]
pub(crate) fn assert_llm_counters(actual: LlmCounterSnapshot, min: LlmCounterMinima) {
    assert!(
        actual.calls >= min.calls,
        "expected at least {} calls, got {}",
        min.calls,
        actual.calls
    );
    assert!(
        actual.input_tokens >= min.input_tokens,
        "expected at least {} input tokens, got {}",
        min.input_tokens,
        actual.input_tokens
    );
    assert!(
        actual.output_tokens >= min.output_tokens,
        "expected at least {} output tokens, got {}",
        min.output_tokens,
        actual.output_tokens
    );
}

#[track_caller]
pub(crate) fn assert_tool_call(call: &ToolCall, expected: ExpectedToolCall<'_>) {
    assert_eq!(call.name, expected.name);
    assert_eq!(call.id, expected.id);
    assert_eq!(call.arguments, serde_json::json!({"key": "value"}));
}
