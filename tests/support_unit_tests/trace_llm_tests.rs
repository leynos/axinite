//! Trace LLM helper tests.

use crate::support::trace_llm::*;
use crate::support::trace_types::TraceTurn;
use ironclaw::llm::{
    ChatMessage, CompletionRequest, FinishReason, LlmProvider, ToolCompletionRequest,
};

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

#[tokio::test]
async fn replays_text_response() {
    let trace = LlmTrace::single_turn("test-model", "hi", vec![text_step("Hello world", 100, 20)]);
    let llm = TraceLlm::from_trace(trace);

    let resp = llm.complete_with_tools(make_request("hi")).await.unwrap();

    assert_eq!(resp.content.as_deref(), Some("Hello world"));
    assert!(resp.tool_calls.is_empty());
    assert_eq!(resp.input_tokens, 100);
    assert_eq!(resp.output_tokens, 20);
    assert_eq!(resp.finish_reason, FinishReason::Stop);
    assert_eq!(llm.calls(), 1);
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
    assert_eq!(resp.tool_calls[0].name, "memory_search");
    assert_eq!(resp.tool_calls[0].id, "call_memory_search");
    assert_eq!(
        resp.tool_calls[0].arguments,
        serde_json::json!({"key": "value"})
    );
    assert_eq!(resp.input_tokens, 80);
    assert_eq!(resp.output_tokens, 15);
    assert_eq!(resp.finish_reason, FinishReason::ToolUse);
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
    assert_eq!(resp1.tool_calls[0].name, "echo");
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
async fn validates_request_hints() {
    let trace = LlmTrace::single_turn(
        "test-model",
        "say hello please",
        vec![TraceStep {
            request_hint: Some(RequestHint {
                last_user_message_contains: Some("hello".to_string()),
                min_message_count: Some(1),
            }),
            response: TraceResponse::Text {
                content: "matched".to_string(),
                input_tokens: 10,
                output_tokens: 5,
            },
            expected_tool_results: Vec::new(),
        }],
    );
    let llm = TraceLlm::from_trace(trace);

    let resp = llm
        .complete_with_tools(make_request("say hello please"))
        .await
        .unwrap();

    assert_eq!(resp.content.as_deref(), Some("matched"));
    assert_eq!(llm.hint_mismatches(), 0);
}

#[tokio::test]
async fn hint_mismatch_warns_but_continues() {
    let trace = LlmTrace::single_turn(
        "test-model",
        "apple",
        vec![TraceStep {
            request_hint: Some(RequestHint {
                last_user_message_contains: Some("banana".to_string()),
                min_message_count: Some(5),
            }),
            response: TraceResponse::Text {
                content: "still works".to_string(),
                input_tokens: 10,
                output_tokens: 5,
            },
            expected_tool_results: Vec::new(),
        }],
    );
    let llm = TraceLlm::from_trace(trace);

    let resp = llm
        .complete_with_tools(make_request("apple"))
        .await
        .unwrap();

    assert_eq!(resp.content.as_deref(), Some("still works"));
    assert_eq!(llm.hint_mismatches(), 2);
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
    assert_eq!(resp.input_tokens, 50);
    assert_eq!(resp.output_tokens, 10);
}

#[tokio::test]
async fn complete_text_step() {
    let trace = LlmTrace::single_turn("test-model", "hi", vec![text_step("plain text", 30, 8)]);
    let llm = TraceLlm::from_trace(trace);

    let resp = llm.complete(make_completion_request("hi")).await.unwrap();

    assert_eq!(resp.content, "plain text");
    assert_eq!(resp.input_tokens, 30);
    assert_eq!(resp.output_tokens, 8);
    assert_eq!(resp.finish_reason, FinishReason::Stop);
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
    assert_eq!(resp.input_tokens, 20);
    assert_eq!(resp.output_tokens, 8);
    assert_eq!(resp.finish_reason, FinishReason::Stop);
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
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].len(), 1);
    assert_eq!(captured[0][0].content, "first message");
    assert_eq!(captured[1][0].content, "second message");
}

#[test]
fn deserialize_flat_steps_as_single_turn() {
    let json = r#"{"model_name": "m", "steps": [
        {"response": {"type": "text", "content": "hi", "input_tokens": 1, "output_tokens": 1}}
    ]}"#;
    let trace: LlmTrace = serde_json::from_str(json).unwrap();
    assert_eq!(trace.turns.len(), 1);
    assert_eq!(trace.turns[0].user_input, "(test input)");
    assert_eq!(trace.turns[0].steps.len(), 1);
}

#[test]
fn deserialize_turns_format() {
    let json = r#"{"model_name": "m", "turns": [
        {"user_input": "hello", "steps": [
            {"response": {"type": "text", "content": "hi", "input_tokens": 1, "output_tokens": 1}}
        ]},
        {"user_input": "bye", "steps": [
            {"response": {"type": "text", "content": "bye", "input_tokens": 1, "output_tokens": 1}}
        ]}
    ]}"#;
    let trace: LlmTrace = serde_json::from_str(json).unwrap();
    assert_eq!(trace.turns.len(), 2);
    assert_eq!(trace.turns[0].user_input, "hello");
    assert_eq!(trace.turns[1].user_input, "bye");
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
