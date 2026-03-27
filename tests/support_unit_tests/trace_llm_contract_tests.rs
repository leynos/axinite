//! Trace LLM request-hint and deserialization contract tests.

use crate::support::trace_llm::*;
use ironclaw::llm::{ChatMessage, LlmProvider, ToolCompletionRequest};

fn make_request(user_msg: &str) -> ToolCompletionRequest {
    ToolCompletionRequest::new(vec![ChatMessage::user(user_msg)], vec![])
}

async fn run_hint_case(
    user_text: &str,
    last_user_contains: &str,
    min_messages: usize,
    response_text: &str,
) -> (String, usize) {
    let trace_json = serde_json::json!({
        "model_name": "test-model",
        "turns": [{
            "user_input": user_text,
            "steps": [{
                "request_hint": {
                    "last_user_message_contains": last_user_contains,
                    "min_message_count": min_messages,
                },
                "response": {
                    "type": "text",
                    "content": response_text,
                    "input_tokens": 10,
                    "output_tokens": 5,
                },
                "expected_tool_results": [],
            }]
        }]
    });
    let trace: LlmTrace =
        serde_json::from_str(&trace_json.to_string()).expect("parse hint test trace");
    let llm = TraceLlm::from_trace(trace);
    let resp = llm
        .complete_with_tools(make_request(user_text))
        .await
        .expect("hint test completion should succeed");

    (
        resp.content
            .expect("hint test response should contain text"),
        llm.hint_mismatches(),
    )
}

macro_rules! hint_test {
    (
        $name:ident,
        user = $user:expr,
        contains = $contains:expr,
        min = $min:expr,
        response = $response:expr,
        expect_mismatches = $expected:expr
    ) => {
        #[tokio::test]
        async fn $name() {
            let (content, mismatches) = run_hint_case($user, $contains, $min, $response).await;
            assert_eq!(content, $response);
            assert_eq!(mismatches, $expected);
        }
    };
}

hint_test!(
    validates_request_hints,
    user = "say hello please",
    contains = "hello",
    min = 1,
    response = "matched",
    expect_mismatches = 0
);

hint_test!(
    validates_request_hints_case_insensitively,
    user = "Write a file to a bad path then recover",
    contains = "write",
    min = 1,
    response = "matched",
    expect_mismatches = 0
);

hint_test!(
    hint_mismatch_warns_but_continues,
    user = "apple",
    contains = "banana",
    min = 5,
    response = "still works",
    expect_mismatches = 2
);

#[test]
fn deserialize_flat_steps_as_single_turn() {
    let json = r#"{"model_name": "m", "steps": [
        {"response": {"type": "text", "content": "hi", "input_tokens": 1, "output_tokens": 1}}
    ]}"#;
    let trace: LlmTrace = serde_json::from_str(json)
        .expect("deserialize_flat_steps_as_single_turn: failed to parse JSON into LlmTrace");
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
    let trace: LlmTrace = serde_json::from_str(json)
        .expect("failed to deserialize LlmTrace in deserialize_turns_format test");
    assert_eq!(trace.turns.len(), 2);
    assert_eq!(trace.turns[0].user_input, "hello");
    assert_eq!(trace.turns[1].user_input, "bye");
}
