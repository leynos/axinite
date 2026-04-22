//! Test rig smoke tests.

use std::time::Duration;

use crate::support::test_rig::TestRigBuilder;
use ironclaw::llm::recording::{TraceResponse, TraceStep};

use crate::support::trace_types::LlmTrace;

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

    let status_events = rig.captured_status_events_async().await;
    assert!(
        !rig.has_safety_warnings(),
        "simple trace should not emit safety warnings: {status_events:?}"
    );
    assert!(
        rig.estimated_cost_usd() >= 0.0,
        "estimated cost should be non-negative"
    );

    rig.clear().await;
    assert!(
        rig.captured_status_events().is_empty(),
        "clear should remove captured status events"
    );
    assert!(
        rig.tool_calls_started().is_empty(),
        "clear should remove captured tool events"
    );

    rig.shutdown();
}
