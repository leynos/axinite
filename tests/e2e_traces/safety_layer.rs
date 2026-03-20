//! E2E trace tests: safety layer.
//!
//! Verifies that the safety layer (injection detection, sanitization) works
//! correctly when enabled in the test rig.

use std::time::Duration;

use crate::support::test_rig::TestRigBuilder;
use crate::support::trace_llm::LlmTrace;

async fn build_injection_rig(
    injection_check: bool,
) -> (LlmTrace, crate::support::test_rig::TestRig) {
    let trace = LlmTrace::from_file_async(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/coverage/injection_in_echo.json"
    ))
    .await
    .expect("failed to load injection_in_echo.json");

    let mut builder = TestRigBuilder::new().with_trace(trace.clone());
    if injection_check {
        builder = builder.with_injection_check(true);
    }
    let rig = builder.build().await.expect("failed to build test rig");
    (trace, rig)
}

/// When injection check is enabled and a tool outputs injection patterns,
/// the safety layer should sanitize the content. The agent must still
/// produce a response and the injection content should not pass through raw.
#[tokio::test]
async fn test_injection_patterns_sanitized() {
    let (trace, rig) = build_injection_rig(true).await;

    rig.send_message("Please echo this text for me").await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(15)).await;

    rig.verify_trace_expects(&trace, &responses);

    // Extra: metrics -- 2 LLM calls (tool + text).
    let metrics = rig.collect_metrics().await;
    assert!(
        metrics.llm_calls >= 2,
        "Expected >= 2 LLM calls, got {}",
        metrics.llm_calls
    );

    rig.shutdown();
}

/// When injection check is disabled (default), tool outputs with injection
/// patterns should still pass through and the agent responds normally.
#[tokio::test]
async fn test_injection_patterns_pass_without_check() {
    let (trace, rig) = build_injection_rig(false).await;

    rig.send_message("Please echo this text for me").await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(15)).await;

    rig.verify_trace_expects(&trace, &responses);
    rig.shutdown();
}
