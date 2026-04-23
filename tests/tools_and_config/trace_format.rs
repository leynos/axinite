//! Trace format / infrastructure tests.
//!
//! These tests verify JSON deserialization and backward compatibility of the
//! trace format. They do NOT require a rig, database, or the `libsql` feature.

use crate::support::trace_llm::{LlmTrace, TraceExpects};
use crate::support::trace_test_files::write_tmp_trace;
use crate::support::trace_types::{TraceTurn, load_trace_with_mutation};
use ironclaw::llm::recording::{TraceResponse, TraceStep};

/// Bundles the expected values checked against a [`TraceExpects`] instance.
struct CoreExpectsSpec<'a> {
    response_contains: &'a [&'a str],
    tools_used: &'a [&'a str],
    all_tools_succeeded: Option<bool>,
    min_responses: Option<usize>,
    echo_result: Option<&'a str>,
}

/// Asserts turn properties at a specific index.
fn assert_turn(turns: &[TraceTurn], idx: usize, input: &str, steps: usize) {
    assert_eq!(turns[idx].user_input, input);
    assert_eq!(turns[idx].steps.len(), steps);
}

/// Asserts empty trace defaults for backward compatibility.
fn assert_empty_trace_defaults(trace: &LlmTrace) {
    assert!(trace.memory_snapshot.is_empty());
    assert!(trace.http_exchanges.is_empty());
    assert!(trace.expects.is_empty());
}

/// Parse a JSON string into an [`LlmTrace`], panicking on failure.
fn parse_trace(json: &str) -> LlmTrace {
    serde_json::from_str(json).expect("failed to parse LlmTrace from JSON")
}

#[tokio::test]
async fn trace_helpers_load_and_mutate_files() -> anyhow::Result<()> {
    let tmp = write_tmp_trace(
        r#"{
            "model_name": "trace-helper",
            "steps": [
                {
                    "response": {
                        "type": "tool_calls",
                        "tool_calls": [{
                            "id": "call_1",
                            "name": "write_file",
                            "arguments": {"path": "__ROOT__/artifact"}
                        }],
                        "input_tokens": 1,
                        "output_tokens": 1
                    }
                }
            ]
        }"#,
    )?;

    let mut trace = LlmTrace::from_file_async(tmp.path()).await?;
    assert_eq!(trace.model_name, "trace-helper");
    let replacement_root = tempfile::tempdir().expect("create replacement root");
    let replacement_root = replacement_root.path().to_string_lossy();
    assert!(
        trace.patch_path("__ROOT__", &replacement_root) >= 1,
        "expected patch_path to rewrite at least one tool-call argument"
    );

    let mutated = load_trace_with_mutation(tmp.path(), |value| {
        value["model_name"] = serde_json::json!("mutated-helper");
    })
    .await?;
    assert_eq!(mutated.model_name, "mutated-helper");

    Ok(())
}

#[test]
fn trace_new_builds_turns() {
    let trace = LlmTrace::new(
        "trace-helper",
        vec![TraceTurn {
            user_input: "hello".to_string(),
            steps: vec![TraceStep {
                request_hint: None,
                response: TraceResponse::Text {
                    content: "world".to_string(),
                    input_tokens: 1,
                    output_tokens: 1,
                },
                expected_tool_results: Vec::new(),
            }],
            expects: TraceExpects::default(),
        }],
    );

    assert_two_turn_trace(
        &LlmTrace::new(
            "trace-helper",
            vec![
                TraceTurn {
                    user_input: "hello".to_string(),
                    steps: trace.turns[0].steps.clone(),
                    expects: TraceExpects::default(),
                },
                TraceTurn {
                    user_input: "again".to_string(),
                    steps: vec![TraceStep {
                        request_hint: None,
                        response: TraceResponse::Text {
                            content: "there".to_string(),
                            input_tokens: 2,
                            output_tokens: 1,
                        },
                        expected_tool_results: Vec::new(),
                    }],
                    expects: TraceExpects::default(),
                },
            ],
        ),
        ("hello", 1),
        ("again", 1),
    );

    let single_turn = LlmTrace::single_turn(
        "trace-helper",
        "one-shot",
        vec![TraceStep {
            request_hint: None,
            response: TraceResponse::Text {
                content: "done".to_string(),
                input_tokens: 1,
                output_tokens: 1,
            },
            expected_tool_results: Vec::new(),
        }],
    );
    assert_eq!(single_turn.turns.len(), 1);
    assert_eq!(single_turn.turns[0].user_input, "one-shot");
}

/// Assert that a trace has exactly two turns with the given inputs and step counts.
fn assert_two_turn_trace(trace: &LlmTrace, turn0: (&str, usize), turn1: (&str, usize)) {
    assert_eq!(trace.turns.len(), 2);
    assert_turn(&trace.turns, 0, turn0.0, turn0.1);
    assert_turn(&trace.turns, 1, turn1.0, turn1.1);
}

/// Asserts core expect fields.
fn assert_core_expects(expects: &TraceExpects, spec: CoreExpectsSpec<'_>) {
    assert_eq!(
        expects.response_contains,
        spec.response_contains
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        expects.tools_used,
        spec.tools_used
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(expects.all_tools_succeeded, spec.all_tools_succeeded);
    assert_eq!(expects.min_responses, spec.min_responses);
    assert_eq!(
        expects.tool_results_contain.get("echo").map(|s| s.as_str()),
        spec.echo_result
    );
}

/// A trace with only user_input steps and no playable steps deserializes.
#[test]
fn all_user_input_steps() {
    let trace = parse_trace(
        r#"{
        "model_name": "recorded-all-user-input",
        "memory_snapshot": [],
        "steps": [
            { "response": { "type": "user_input", "content": "hello" } },
            { "response": { "type": "user_input", "content": "world" } }
        ]
    }"#,
    );
    assert_eq!(trace.steps.len(), 2);
    assert_eq!(trace.playable_steps().len(), 0);
}

/// Backward compatibility: a trace without the new fields loads correctly.
#[test]
fn backward_compat_no_memory_snapshot() {
    let trace = parse_trace(
        r#"{
        "model_name": "old-format",
        "steps": [
            {
                "response": {
                    "type": "text",
                    "content": "hello",
                    "input_tokens": 10,
                    "output_tokens": 5
                }
            }
        ]
    }"#,
    );
    assert_empty_trace_defaults(&trace);
    assert_eq!(trace.playable_steps().len(), 1);
}

/// Expects round-trips through JSON serialization.
#[test]
fn expects_deserialization() {
    let trace = parse_trace(
        r#"{
        "model_name": "expects-test",
        "expects": {
            "response_contains": ["hello", "world"],
            "tools_used": ["echo"],
            "all_tools_succeeded": true,
            "min_responses": 1,
            "tool_results_contain": { "echo": "greeting" }
        },
        "steps": [
            {
                "response": {
                    "type": "text",
                    "content": "hello world",
                    "input_tokens": 10,
                    "output_tokens": 5
                }
            }
        ]
    }"#,
    );
    assert!(!trace.expects.is_empty());
    assert_core_expects(
        &trace.expects,
        CoreExpectsSpec {
            response_contains: &["hello", "world"],
            tools_used: &["echo"],
            all_tools_succeeded: Some(true),
            min_responses: Some(1),
            echo_result: Some("greeting"),
        },
    );

    // Round-trip: serialize back and deserialize again.
    let serialized = serde_json::to_string(&trace).expect("failed to serialize LlmTrace");
    let trace2: LlmTrace =
        serde_json::from_str(&serialized).expect("failed to deserialize LlmTrace");
    assert_eq!(
        trace2.expects.response_contains,
        trace.expects.response_contains
    );
    assert_eq!(trace2.expects.tools_used, trace.expects.tools_used);
}

/// A trace without `expects` loads with empty defaults.
#[test]
fn expects_default_empty() {
    let trace = parse_trace(
        r#"{
        "model_name": "no-expects",
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
    }"#,
    );
    assert!(trace.expects.is_empty());
}

/// Per-turn expects deserializes correctly.
#[test]
fn per_turn_expects() {
    let trace = parse_trace(
        r#"{
        "model_name": "turn-expects",
        "turns": [
            {
                "user_input": "hello",
                "expects": {
                    "response_contains": ["greeting"],
                    "tools_not_used": ["shell"]
                },
                "steps": [
                    {
                        "response": {
                            "type": "text",
                            "content": "greeting back",
                            "input_tokens": 1,
                            "output_tokens": 1
                        }
                    }
                ]
            }
        ]
    }"#,
    );
    assert_eq!(trace.turns.len(), 1);
    assert_eq!(trace.turns[0].expects.response_contains, vec!["greeting"]);
    assert_eq!(trace.turns[0].expects.tools_not_used, vec!["shell"]);
}

/// TraceExpects::is_empty() returns true for default.
#[test]
fn trace_expects_is_empty() {
    let e = TraceExpects::default();
    assert!(e.is_empty());
}

/// Flat steps with UserInput markers are split into multiple turns.
#[test]
fn recorded_multi_turn_splits_at_user_input() {
    let trace = parse_trace(
        r#"{
        "model_name": "test",
        "steps": [
            { "response": { "type": "user_input", "content": "hello" } },
            { "response": { "type": "text", "content": "hi", "input_tokens": 10, "output_tokens": 5 } },
            { "response": { "type": "user_input", "content": "bye" } },
            { "response": { "type": "text", "content": "goodbye", "input_tokens": 20, "output_tokens": 5 } }
        ]
    }"#,
    );
    assert_two_turn_trace(&trace, ("hello", 1), ("bye", 1));
}

/// Steps before the first UserInput get placeholder input.
#[test]
fn steps_before_first_user_input_get_placeholder() {
    let trace = parse_trace(
        r#"{
        "model_name": "test",
        "steps": [
            { "response": { "type": "text", "content": "preamble", "input_tokens": 5, "output_tokens": 3 } },
            { "response": { "type": "user_input", "content": "hello" } },
            { "response": { "type": "text", "content": "hi", "input_tokens": 10, "output_tokens": 5 } }
        ]
    }"#,
    );
    assert_two_turn_trace(&trace, ("(test input)", 1), ("hello", 1));
}
