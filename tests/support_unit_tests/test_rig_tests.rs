//! Test rig smoke tests.

use std::time::Duration;

use crate::support::test_rig::TestRigBuilder;
use crate::support::trace_llm::{LlmTrace, TraceResponse, TraceStep};

#[tokio::test]
async fn rig_builds_and_runs() {
    let trace = LlmTrace::single_turn(
        "test-model",
        "Hello test rig",
        vec![TraceStep {
            request_hint: None,
            response: TraceResponse::Text {
                content: "I am the test rig response.".to_string(),
                input_tokens: 50,
                output_tokens: 15,
            },
            expected_tool_results: Vec::new(),
        }],
    );

    let rig = TestRigBuilder::new()
        .with_trace(trace)
        .build()
        .await
        .expect("failed to build test rig");

    rig.send_message("Hello test rig").await;

    let responses = rig.wait_for_responses(1, Duration::from_secs(10)).await;

    assert!(
        !responses.is_empty(),
        "Expected at least one response from the agent"
    );
    let found = responses
        .iter()
        .any(|r| r.content.contains("I am the test rig response."));
    assert!(
        found,
        "Expected a response containing the trace text, got: {:?}",
        responses.iter().map(|r| &r.content).collect::<Vec<_>>()
    );

    rig.shutdown();
}
