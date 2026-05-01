//! Shared fixtures for TraceLlm unit tests.

use ironclaw::llm::recording::{TraceResponse, TraceStep, TraceToolCall};
use ironclaw::llm::{ChatMessage, CompletionRequest, ToolCompletionRequest};

use crate::support::trace_provider::TraceLlm;
use crate::support::trace_types::LlmTrace;

/// Builds a text-response trace step.
///
/// `content`, `input_tokens`, and `output_tokens` populate the response; the
/// returned [`TraceStep`] has no request hint or expected tool results.
pub fn text_step(content: &str, input_tokens: u32, output_tokens: u32) -> TraceStep {
    TraceStep {
        request_hint: None,
        response: TraceResponse::Text {
            content: content.to_string(),
            input_tokens,
            output_tokens,
        },
        expected_tool_results: Vec::new(),
    }
}

/// Builds a tool-call trace step.
///
/// `calls` becomes the response tool-call list, while `input` and `output`
/// populate the token counts on the returned [`TraceStep`].
pub fn tool_calls_step(calls: Vec<TraceToolCall>, input: u32, output: u32) -> TraceStep {
    TraceStep {
        request_hint: None,
        response: TraceResponse::ToolCalls {
            tool_calls: calls,
            input_tokens: input,
            output_tokens: output,
        },
        expected_tool_results: Vec::new(),
    }
}

/// Builds a simple trace tool call with fixed JSON arguments.
///
/// `name` is used for the tool name and to derive the returned call id.
pub fn simple_tool_call(name: &str) -> TraceToolCall {
    TraceToolCall {
        id: format!("call_{name}"),
        name: name.to_string(),
        arguments: serde_json::json!({"key": "value"}),
    }
}

/// Builds a tool-completion request containing one user message.
///
/// `user_msg` becomes the sole user message; the returned request has no tool
/// definitions.
pub fn make_request(user_msg: &str) -> ToolCompletionRequest {
    ToolCompletionRequest::new(vec![ChatMessage::user(user_msg)], vec![])
}

/// Builds a plain completion request containing one user message.
///
/// `user_msg` becomes the sole user message in the returned
/// [`CompletionRequest`].
pub fn make_completion_request(user_msg: &str) -> CompletionRequest {
    CompletionRequest::new(vec![ChatMessage::user(user_msg)])
}

/// Builds a [`TraceLlm`] backed by a single text-response step.
///
/// `user_msg` seeds the trace turn, while `content`, `input_tokens`, and
/// `output_tokens` configure the returned provider's only replayable response.
pub fn single_text_step_llm(
    user_msg: &str,
    content: &str,
    input_tokens: u32,
    output_tokens: u32,
) -> TraceLlm {
    let trace = LlmTrace::single_turn(
        "test-model",
        user_msg,
        vec![text_step(content, input_tokens, output_tokens)],
    );
    TraceLlm::from_trace(trace)
}
