//! Trace LLM helper tests.

use crate::support::trace_llm::*;
use crate::support::trace_types::TraceTurn;
use ironclaw::llm::{
    ChatMessage, CompletionRequest, FinishReason, LlmProvider, Role, ToolCall,
    ToolCompletionRequest,
};

#[derive(Copy, Clone, Debug)]
struct LlmCounterSnapshot {
    calls: usize,
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Copy, Clone, Debug)]
struct LlmCounterMinima {
    calls: usize,
    input_tokens: u32,
    output_tokens: u32,
}

fn assert_msg(role: Role, msg: &ChatMessage, contains: &str) {
    assert_eq!(msg.role, role);
    assert!(
        msg.content.contains(contains),
        "expected message content {:?} to contain {:?}",
        msg.content,
        contains
    );
}

fn assert_captured_requests_shape(
    captured: &[Vec<ChatMessage>],
    expected_batches: usize,
    last_user_contains: &str,
    min_msgs_per_batch: usize,
) {
    assert_eq!(captured.len(), expected_batches);
    assert!(
        captured
            .iter()
            .all(|batch| batch.len() >= min_msgs_per_batch),
        "expected every captured batch to contain at least {min_msgs_per_batch} messages"
    );
    let last_batch = captured
        .last()
        .expect("captured requests should contain at least one batch");
    let last_message = last_batch
        .last()
        .expect("captured request batch should contain at least one message");
    assert_msg(Role::User, last_message, last_user_contains);
}

fn assert_llm_counters(actual: LlmCounterSnapshot, min: LlmCounterMinima) {
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

fn assert_tool_call(call: &ToolCall, expected_name: &str, expected_id: &str) {
    assert_eq!(call.name, expected_name);
    assert_eq!(call.id, expected_id);
    assert_eq!(call.arguments, serde_json::json!({"key": "value"}));
}

fn text_step(content: &str, input_tokens: u32, output_tokens: u32) -> TraceStep {
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

fn tool_calls_step(calls: Vec<TraceToolCall>, input: u32, output: u32) -> TraceStep {
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

fn simple_tool_call(name: &str) -> TraceToolCall {
    TraceToolCall {
        id: format!("call_{name}"),
        name: name.to_string(),
        arguments: serde_json::json!({"key": "value"}),
    }
}

fn make_request(user_msg: &str) -> ToolCompletionRequest {
    ToolCompletionRequest::new(vec![ChatMessage::user(user_msg)], vec![])
}

fn make_completion_request(user_msg: &str) -> CompletionRequest {
    CompletionRequest::new(vec![ChatMessage::user(user_msg)])
}

fn single_text_step_llm(
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

#[tokio::test]
async fn replays_text_response() {
    let llm = single_text_step_llm("hi", "Hello world", 100, 20);

    let resp = llm.complete_with_tools(make_request("hi")).await.unwrap();

    assert_eq!(resp.content.as_deref(), Some("Hello world"));
    assert!(resp.tool_calls.is_empty());
    assert_eq!(resp.finish_reason, FinishReason::Stop);
    assert_llm_counters(
        LlmCounterSnapshot {
            calls: llm.calls(),
            input_tokens: resp.input_tokens,
            output_tokens: resp.output_tokens,
        },
        LlmCounterMinima {
            calls: 1,
            input_tokens: 100,
            output_tokens: 20,
        },
    );
}

#[tokio::test]
async fn replays_tool_calls() {
    let trace = LlmTrace::single_turn(
        "test-model",
        "search memory",
        vec![tool_calls_step(
            vec![simple_tool_call("memory_search")],
            80,
            15,
        )],
    );
    let llm = TraceLlm::from_trace(trace);

    let resp = llm
        .complete_with_tools(make_request("search memory"))
        .await
        .unwrap();

    assert!(resp.content.is_none());
    assert_eq!(resp.tool_calls.len(), 1);
    assert_tool_call(&resp.tool_calls[0], "memory_search", "call_memory_search");
    assert_eq!(resp.finish_reason, FinishReason::ToolUse);
    assert_llm_counters(
        LlmCounterSnapshot {
            calls: 1,
            input_tokens: resp.input_tokens,
            output_tokens: resp.output_tokens,
        },
        LlmCounterMinima {
            calls: 1,
            input_tokens: 80,
            output_tokens: 15,
        },
    );
}

#[tokio::test]
async fn advances_through_steps() {
    let trace = LlmTrace::single_turn(
        "test-model",
        "do something",
        vec![
            tool_calls_step(vec![simple_tool_call("echo")], 50, 10),
            text_step("Done!", 60, 5),
        ],
    );
    let llm = TraceLlm::from_trace(trace);

    let resp1 = llm
        .complete_with_tools(make_request("do something"))
        .await
        .unwrap();
    assert_eq!(resp1.tool_calls.len(), 1);
    assert_tool_call(&resp1.tool_calls[0], "echo", "call_echo");
    assert_eq!(llm.calls(), 1);

    let resp2 = llm
        .complete_with_tools(make_request("continue"))
        .await
        .unwrap();
    assert_eq!(resp2.content.as_deref(), Some("Done!"));
    assert!(resp2.tool_calls.is_empty());
    assert_eq!(llm.calls(), 2);
}

#[tokio::test]
async fn errors_when_exhausted() {
    let trace = LlmTrace::single_turn("test-model", "first", vec![text_step("only once", 10, 5)]);
    let llm = TraceLlm::from_trace(trace);

    let resp1 = llm.complete_with_tools(make_request("first")).await;
    assert!(resp1.is_ok());

    let resp2 = llm.complete_with_tools(make_request("second")).await;
    assert!(resp2.is_err());
    let err = resp2.unwrap_err();
    let err_msg = err.to_string();
    assert!(
        err_msg.contains("exhausted"),
        "Expected 'exhausted' in error: {err_msg}"
    );
}


#[tokio::test]
async fn from_json_file() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/simple_text.json"
    );
    let llm = TraceLlm::from_file_async(fixture_path)
        .await
        .expect("failed to create TraceLlm from fixture_path");

    assert_eq!(llm.model_name(), "test-model");

    let resp = llm
        .complete_with_tools(make_request("anything"))
        .await
        .unwrap();

    assert_eq!(resp.content.as_deref(), Some("Hello from fixture file!"));
    assert_llm_counters(
        LlmCounterSnapshot {
            calls: 0,
            input_tokens: resp.input_tokens,
            output_tokens: resp.output_tokens,
        },
        LlmCounterMinima {
            calls: 0,
            input_tokens: 50,
            output_tokens: 10,
        },
    );
}

#[tokio::test]
async fn complete_text_step() {
    let llm = single_text_step_llm("hi", "plain text", 30, 8);

    let resp = llm.complete(make_completion_request("hi")).await.unwrap();

    assert_eq!(resp.content, "plain text");
    assert_eq!(resp.finish_reason, FinishReason::Stop);
    assert_llm_counters(
        LlmCounterSnapshot {
            calls: 0,
            input_tokens: resp.input_tokens,
            output_tokens: resp.output_tokens,
        },
        LlmCounterMinima {
            calls: 0,
            input_tokens: 30,
            output_tokens: 8,
        },
    );
}

#[tokio::test]
async fn complete_skips_tool_calls_step() {
    let trace = LlmTrace::single_turn(
        "test-model",
        "hi",
        vec![
            tool_calls_step(vec![simple_tool_call("echo")], 10, 5),
            text_step("skipped past tools", 20, 8),
        ],
    );
    let llm = TraceLlm::from_trace(trace);

    let resp = llm
        .complete(make_completion_request("hi"))
        .await
        .expect("complete() should skip ToolCalls and return the Text step");

    assert_eq!(resp.content, "skipped past tools");
    assert_eq!(resp.finish_reason, FinishReason::Stop);
    assert_llm_counters(
        LlmCounterSnapshot {
            calls: 0,
            input_tokens: resp.input_tokens,
            output_tokens: resp.output_tokens,
        },
        LlmCounterMinima {
            calls: 0,
            input_tokens: 20,
            output_tokens: 8,
        },
    );
}

#[tokio::test]
async fn captured_requests() {
    let trace = LlmTrace::single_turn(
        "test-model",
        "test",
        vec![text_step("resp1", 10, 5), text_step("resp2", 10, 5)],
    );
    let llm = TraceLlm::from_trace(trace);

    llm.complete_with_tools(make_request("first message"))
        .await
        .unwrap();
    llm.complete_with_tools(make_request("second message"))
        .await
        .unwrap();

    let captured = llm
        .captured_requests()
        .expect("captured requests should be available");
    assert_captured_requests_shape(&captured, 2, "second message", 1);
    assert_msg(Role::User, &captured[0][0], "first message");
}

#[tokio::test]
async fn multi_turn() {
    let trace = LlmTrace::new(
        "turns-model",
        vec![
            TraceTurn {
                user_input: "first".to_string(),
                steps: vec![text_step("turn 1 response", 10, 5)],
                expects: TraceExpects::default(),
            },
            TraceTurn {
                user_input: "second".to_string(),
                steps: vec![text_step("turn 2 response", 20, 10)],
                expects: TraceExpects::default(),
            },
        ],
    );
    let llm = TraceLlm::from_trace(trace);

    let resp1 = llm
        .complete_with_tools(make_request("first"))
        .await
        .unwrap();
    assert_eq!(resp1.content.as_deref(), Some("turn 1 response"));

    let resp2 = llm
        .complete_with_tools(make_request("second"))
        .await
        .unwrap();
    assert_eq!(resp2.content.as_deref(), Some("turn 2 response"));

    assert_eq!(llm.calls(), 2);
}
