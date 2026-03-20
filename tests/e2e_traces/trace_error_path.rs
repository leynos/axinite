//! E2E trace test: tool error path.
//!
//! Validates that the agent handles tool errors gracefully (no crash)
//! when a tool call is made with missing required parameters.

use std::time::Duration;

use crate::support::test_rig::TestRigBuilder;
use crate::support::trace_llm::LlmTrace;

#[tokio::test]
async fn test_tool_error_handled_gracefully() {
    let trace = LlmTrace::from_file_async(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/error_path.json"
    ))
    .await
    .expect("failed to load error_path.json trace fixture");

    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .build()
        .await
        .expect("failed to build test rig");

    rig.send_message("Read a file for me").await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(15)).await;

    rig.verify_trace_expects(&trace, &responses);
    rig.shutdown();
}
