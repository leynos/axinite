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

/// Run a test verifying that specific tools were started.
pub async fn run_routine_started_test(fixture_path: &str, message: &str, expected_tools: &[&str]) {
    let (rig, _trace, _responses) = run_trace_test(
        fixture_path,
        message,
        RigConfig {
            routines: true,
            ..RigConfig::default()
        },
    )
    .await;
    let started = rig.tool_calls_started();
    for tool in expected_tools {
        assert!(
            started.contains(&(*tool).to_string()),
            "{tool} not started: {started:?}"
        );
    }

    rig.shutdown();
}

/// Macro for generating routine started tests.
#[macro_export]
macro_rules! routine_started_test {
    ($name:ident, $fixture:literal, $message:literal, [$($tool:literal),+ $(,)?]) => {
        #[tokio::test]
        async fn $name() {
            run_routine_started_test(
                concat!(env!("CARGO_MANIFEST_DIR"), $fixture),
                $message,
                &[$($tool),+],
            )
            .await;
        }
    };
}

// Re-export the macro for internal use
#[allow(unused_imports)]
pub use routine_started_test;
