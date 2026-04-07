//! Common test helpers for builtin tool coverage tests.

use std::time::Duration;

use ironclaw::channels::OutgoingResponse;

use crate::support::test_rig::{TestRig, TestRigBuilder};
use crate::support::trace_llm::LlmTrace;

/// Error type for test harness operations.
#[derive(Debug)]
pub enum HarnessError {
    /// Failed to load trace file.
    TraceLoad(String),
    /// Failed to build test rig.
    RigBuild(String),
}

impl std::fmt::Display for HarnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HarnessError::TraceLoad(msg) => write!(f, "{msg}"),
            HarnessError::RigBuild(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for HarnessError {}

/// Configuration for test rig setup.
pub struct RigConfig {
    /// Whether to auto-approve tool calls.
    pub auto_approve: bool,
    /// Whether to enable routines.
    pub routines: bool,
    /// Whether to enable skills.
    pub skills: bool,
}

impl Default for RigConfig {
    fn default() -> Self {
        Self {
            auto_approve: true,
            routines: false,
            skills: false,
        }
    }
}

/// Run a trace test with a default 15-second timeout.
pub async fn run_trace_test(
    fixture_path: &str,
    message: &str,
    config: RigConfig,
) -> Result<(TestRig, LlmTrace, Vec<OutgoingResponse>), HarnessError> {
    run_trace_test_with_timeout(fixture_path, message, config, Duration::from_secs(15)).await
}

/// Run a trace test with a configurable timeout.
pub async fn run_trace_test_with_timeout(
    fixture_path: &str,
    message: &str,
    config: RigConfig,
    timeout: Duration,
) -> Result<(TestRig, LlmTrace, Vec<OutgoingResponse>), HarnessError> {
    let trace = LlmTrace::from_file_async(fixture_path)
        .await
        .map_err(|e| HarnessError::TraceLoad(format!("failed to load {fixture_path}: {e}")))?;

    let mut builder = TestRigBuilder::new()
        .with_trace(trace.clone())
        .with_auto_approve_tools(config.auto_approve);
    if config.skills {
        builder = builder.with_skills();
    }
    if config.routines {
        builder = builder.with_routines();
    }
    let rig = builder
        .build()
        .await
        .map_err(|e| HarnessError::RigBuild(format!("failed to build test rig: {e}")))?;

    rig.send_message(message).await;
    let responses = rig.wait_for_responses(1, timeout).await;

    rig.verify_trace_expects(&trace, &responses);
    Ok((rig, trace, responses))
}
