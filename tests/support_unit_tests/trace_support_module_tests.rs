//! Focused unit tests for split trace and webhook support helpers.

use std::io::Write;

use ironclaw::llm::recording::{TraceResponse, TraceStep, TraceToolCall};

use crate::support::trace_provider::TraceLlm;
use crate::support::trace_types::{LlmTrace, TraceExpects, TraceTurn};
use crate::support::webhook_server_helpers::start_health_server;

fn text_step(content: &str) -> TraceStep {
    TraceStep {
        request_hint: None,
        response: TraceResponse::Text {
            content: content.to_string(),
            input_tokens: 1,
            output_tokens: 1,
        },
        expected_tool_results: Vec::new(),
    }
}

fn user_input_step(content: &str) -> TraceStep {
    TraceStep {
        request_hint: None,
        response: TraceResponse::UserInput {
            content: content.to_string(),
        },
        expected_tool_results: Vec::new(),
    }
}

fn tool_call_step(path: &str) -> TraceStep {
    TraceStep {
        request_hint: None,
        response: TraceResponse::ToolCalls {
            tool_calls: vec![TraceToolCall {
                id: "call_write".to_string(),
                name: "write_file".to_string(),
                arguments: serde_json::json!({
                    "path": path,
                    "content": "hello"
                }),
            }],
            input_tokens: 3,
            output_tokens: 2,
        },
        expected_tool_results: Vec::new(),
    }
}

fn minimal_trace_json() -> serde_json::Value {
    serde_json::json!({
        "model_name": "trace-model",
        "turns": [
            {
                "user_input": "hello",
                "steps": [
                    {
                        "response": {
                            "type": "text",
                            "content": "hi",
                            "input_tokens": 1,
                            "output_tokens": 1
                        }
                    }
                ]
            }
        ]
    })
}

fn write_trace_fixture() -> tempfile::NamedTempFile {
    let json = minimal_trace_json();
    let mut file = tempfile::NamedTempFile::new().expect("should create temp trace fixture");
    write!(file, "{json}").expect("should write trace fixture");
    file
}

#[test]
fn llm_trace_new_initialises_empty_optional_fields() {
    let turns = vec![TraceTurn {
        user_input: "hello".to_string(),
        steps: vec![text_step("world")],
        expects: TraceExpects::default(),
    }];

    let trace = LlmTrace::new("builder-model", turns.clone());

    assert_eq!(trace.model_name, "builder-model");
    assert_eq!(trace.turns.len(), 1);
    assert_eq!(trace.turns[0].user_input, turns[0].user_input);
    assert_eq!(trace.turns[0].steps.len(), turns[0].steps.len());
    assert!(trace.memory_snapshot.is_empty());
    assert!(trace.http_exchanges.is_empty());
    assert!(trace.expects.is_empty());
    assert!(trace.steps.is_empty());
}

#[test]
fn llm_trace_single_turn_builds_one_turn_trace() {
    let steps = vec![text_step("done")];

    let trace = LlmTrace::single_turn("runtime-model", "run once", steps.clone());

    assert_eq!(trace.model_name, "runtime-model");
    assert_eq!(trace.turns.len(), 1);
    assert_eq!(trace.turns[0].user_input, "run once");
    assert_eq!(trace.turns[0].steps.len(), steps.len());
    match &trace.turns[0].steps[0].response {
        TraceResponse::Text { content, .. } => assert_eq!(content, "done"),
        other => panic!("expected text step, got {other:?}"),
    }
}

#[tokio::test]
async fn llm_trace_from_file_async_reads_fixture() {
    let file = write_trace_fixture();

    let trace = LlmTrace::from_file_async(file.path())
        .await
        .expect("from_file_async should load fixture");

    assert_eq!(trace.model_name, "trace-model");
    assert_eq!(trace.turns.len(), 1);
    assert_eq!(trace.turns[0].user_input, "hello");
    assert_eq!(trace.turns[0].steps.len(), 1);
}

#[test]
fn llm_trace_patch_path_rewrites_tool_call_arguments() {
    let from = "/tmp/original";
    let to = "/tmp/rewritten";
    let mut trace = LlmTrace::single_turn("patch-model", "patch file", vec![tool_call_step(from)]);

    let patched = trace.patch_path(from, to);

    assert_eq!(patched, 1);
    let TraceResponse::ToolCalls { tool_calls, .. } = &trace.turns[0].steps[0].response else {
        panic!("expected tool call step after patch");
    };
    assert_eq!(tool_calls[0].arguments["path"], serde_json::json!(to));
}

#[test]
fn llm_trace_patch_path_returns_zero_without_tool_calls() {
    let mut trace = LlmTrace::single_turn("patch-model", "no patch", vec![text_step("done")]);

    let patched = trace.patch_path("/tmp/original", "/tmp/rewritten");

    assert_eq!(patched, 0);
}

#[test]
fn llm_trace_patch_path_ignores_empty_from_pattern() {
    let original = "/tmp/original";
    let mut trace =
        LlmTrace::single_turn("patch-model", "patch file", vec![tool_call_step(original)]);

    let patched = trace.patch_path("", "/tmp/rewritten");

    assert_eq!(patched, 0);
    let TraceResponse::ToolCalls { tool_calls, .. } = &trace.turns[0].steps[0].response else {
        panic!("expected tool call step after empty patch");
    };
    assert_eq!(tool_calls[0].arguments["path"], serde_json::json!(original));
}

#[test]
fn playable_steps_skips_user_input_markers() {
    let playable = text_step("play me");
    let trace = LlmTrace {
        model_name: "recorded-model".to_string(),
        turns: Vec::new(),
        memory_snapshot: Vec::new(),
        http_exchanges: Vec::new(),
        expects: TraceExpects::default(),
        steps: vec![user_input_step("hello"), playable.clone()],
    };

    let steps = trace.playable_steps();

    assert_eq!(steps.len(), 1);
    match &steps[0].response {
        TraceResponse::Text { content, .. } => assert_eq!(content, "play me"),
        other => panic!("expected text step, got {other:?}"),
    }
}

#[test]
fn trace_llm_diagnostics_start_at_zero() {
    let llm = TraceLlm::from_trace(LlmTrace::single_turn(
        "diag-model",
        "hello",
        vec![text_step("hi")],
    ));

    assert_eq!(llm.calls(), 0);
    assert_eq!(llm.hint_mismatches(), 0);
}

#[tokio::test]
async fn trace_llm_from_file_async_loads_fixture() {
    let file = write_trace_fixture();

    let llm = TraceLlm::from_file_async(file.path()).await;

    assert!(llm.is_ok(), "TraceLlm::from_file_async should load fixture");
}

#[tokio::test]
async fn start_health_server_serves_health_route() {
    let mut started = start_health_server()
        .await
        .expect("start_health_server should succeed");

    assert!(
        started.addr.port() > 0,
        "ephemeral bind should choose a port"
    );

    let response = started
        .client
        .get(format!("http://{}/health", started.addr))
        .send()
        .await
        .expect("health request should succeed");

    assert_eq!(response.status(), reqwest::StatusCode::OK);

    started.server.shutdown().await;
}
