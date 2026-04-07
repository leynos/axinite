//! Common test helpers for builtin tool coverage tests.

use std::time::Duration;

use ironclaw::channels::OutgoingResponse;

use crate::support::test_rig::{TestRig, TestRigBuilder};
use crate::support::trace_llm::LlmTrace;

/// Configuration for test rig setup.
#[derive(Default)]
pub struct RigConfig {
    /// Whether to auto-approve tool calls.
    pub auto_approve: bool,
    /// Whether to enable routines.
    pub routines: bool,
    /// Whether to enable skills.
    pub skills: bool,
}

/// Run a trace test with a default 15-second timeout.
pub async fn run_trace_test(
    fixture_path: &str,
    message: &str,
    config: RigConfig,
) -> (TestRig, LlmTrace, Vec<OutgoingResponse>) {
    run_trace_test_with_timeout(fixture_path, message, config, Duration::from_secs(15)).await
}

/// Run a trace test with a configurable timeout.
pub async fn run_trace_test_with_timeout(
    fixture_path: &str,
    message: &str,
    config: RigConfig,
    timeout: Duration,
) -> (TestRig, LlmTrace, Vec<OutgoingResponse>) {
    let load_error = format!("failed to load {fixture_path}");
    let trace = LlmTrace::from_file_async(fixture_path)
        .await
        .expect(load_error.as_str());

    let mut builder = TestRigBuilder::new().with_trace(trace.clone());
    if config.auto_approve {
        builder = builder.with_auto_approve_tools(true);
    }
    if config.skills {
        builder = builder.with_skills();
    }
    if config.routines {
        builder = builder.with_routines();
    }
    let rig = builder.build().await.expect("failed to build test rig");

    rig.send_message(message).await;
    let responses = rig.wait_for_responses(1, timeout).await;

    rig.verify_trace_expects(&trace, &responses);
    (rig, trace, responses)
}
