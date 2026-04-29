//! Shared fixtures for TraceLlm unit tests.

use ironclaw::llm::recording::{TraceResponse, TraceStep, TraceToolCall};
use ironclaw::llm::{ChatMessage, CompletionRequest, ToolCompletionRequest};

use crate::support::trace_provider::TraceLlm;
use crate::support::trace_types::LlmTrace;

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

pub fn simple_tool_call(name: &str) -> TraceToolCall {
    TraceToolCall {
        id: format!("call_{name}"),
        name: name.to_string(),
        arguments: serde_json::json!({"key": "value"}),
    }
}

pub fn make_request(user_msg: &str) -> ToolCompletionRequest {
    ToolCompletionRequest::new(vec![ChatMessage::user(user_msg)], vec![])
}

pub fn make_completion_request(user_msg: &str) -> CompletionRequest {
    CompletionRequest::new(vec![ChatMessage::user(user_msg)])
}

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
