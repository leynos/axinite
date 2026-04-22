//! E2E trace tests: worker execution paths (#571).
//!
//! Covers parallel tool calls, error feedback loops, unknown tools,
//! invalid parameters, rate limiting, iteration limits, and planning mode.

use std::sync::Arc;
use std::time::Duration;

use rstest::rstest;
use serde_json::json;

use ironclaw::channels::OutgoingResponse;
use ironclaw::context::JobContext;
use ironclaw::tools::{NativeTool, Tool, ToolError, ToolOutput};

use crate::support::test_rig::{TestRig, TestRigBuilder};
use crate::support::trace_llm::LlmTrace;
use crate::support::trace_types::load_trace_with_mutation;

// -- Stub tools for rate-limit and timeout tests --------------------------

/// A tool that always returns RateLimited.
struct StubRateLimitTool;

impl NativeTool for StubRateLimitTool {
    fn name(&self) -> &str {
        "stub_rate_limit"
    }
    fn description(&self) -> &str {
        "Always returns rate limited error"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        Err(ToolError::RateLimited(Some(Duration::from_secs(60))))
    }
}

#[derive(Debug)]
struct SpotCase {
    fixture: &'static str,
    message: &'static str,
}

async fn run_worker_spot_case(
    fixture_path: &str,
    message: &str,
) -> anyhow::Result<(LlmTrace, Vec<OutgoingResponse>, TestRig)> {
    let trace = LlmTrace::from_file_async(fixture_path).await?;
    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .build()
        .await?;
    rig.send_message(message).await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(15)).await;
    rig.verify_trace_expects(&trace, &responses);
    Ok((trace, responses, rig))
}

async fn run_worker_spot_test(fixture_path: &str, message: &str) -> anyhow::Result<()> {
    let (_trace, _responses, rig) = run_worker_spot_case(fixture_path, message).await?;
    rig.shutdown();
    Ok(())
}

// -----------------------------------------------------------------------
// Test 1: baseline worker spot checks
// -----------------------------------------------------------------------

#[rstest]
#[case(SpotCase {
    fixture: concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/worker/parallel_three_tools.json"
    ),
    message: "Run three tools in parallel",
})]
#[case(SpotCase {
    fixture: concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/worker/unknown_tool.json"
    ),
    message: "Deploy to production",
})]
#[case(SpotCase {
    fixture: concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/worker/invalid_params.json"
    ),
    message: "Echo something with wrong params first",
})]
#[case(SpotCase {
    fixture: concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/worker/plan_remaining_work.json"
    ),
    message: "Plan and execute a task",
})]
#[tokio::test]
async fn worker_spot(#[case] case: SpotCase) -> anyhow::Result<()> {
    run_worker_spot_test(case.fixture, case.message).await
}

#[tokio::test]
async fn parallel_three_tools_starts_all_tools() {
    let (_trace, _responses, rig) = run_worker_spot_case(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/llm_traces/worker/parallel_three_tools.json"
        ),
        "Run three tools in parallel",
    )
    .await
    .expect("failed to run worker spot case");

    let started = rig.tool_calls_started();
    assert!(
        started.contains(&"echo".to_string()),
        "echo not started: {started:?}"
    );
    assert!(
        started.contains(&"time".to_string()),
        "time not started: {started:?}"
    );
    assert!(
        started.contains(&"json".to_string()),
        "json not started: {started:?}"
    );

    rig.shutdown();
}

// -----------------------------------------------------------------------
// Test 2: tool_error_feedback
// -----------------------------------------------------------------------

#[tokio::test]
async fn tool_error_feedback() {
    // Use a tempdir for the recovery file. The fixture's recovery path
    // is updated to write here via the test_dir variable.
    let tmp = tempfile::tempdir().expect("create temp dir");
    let test_dir = tmp.path().to_str().expect("tempdir path");

    // Load and patch the fixture's recovery path to use our tempdir.
    let trace = load_trace_with_mutation(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/llm_traces/worker/tool_error_feedback.json"
        ),
        |value| {
            let recovery_path =
                &mut value["steps"][1]["response"]["tool_calls"][0]["arguments"]["path"];
            assert!(
                recovery_path.is_string(),
                "expected worker/tool_error_feedback.json recovery path to be a string"
            );
            *recovery_path = serde_json::Value::String(format!("{test_dir}/recovered.txt"));
        },
    )
    .await
    .expect("failed to load worker/tool_error_feedback.json");

    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .build()
        .await
        .expect("failed to build test rig");

    rig.send_message("write a file to a bad path then recover")
        .await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(15)).await;

    rig.verify_trace_expects(&trace, &responses);

    // Verify the recovery file exists in the tempdir.
    let content = std::fs::read_to_string(format!("{test_dir}/recovered.txt"))
        .expect("recovered.txt should exist");
    assert!(
        content.contains("recovered"),
        "Expected 'recovered' in file, got: {content:?}"
    );

    // At least one tool call should have failed (the bad path).
    let completed = rig.tool_calls_completed();
    let failures: Vec<_> = completed.iter().filter(|(_, ok)| !ok).collect();
    assert!(
        !failures.is_empty(),
        "Expected at least one failed tool call, got: {completed:?}"
    );

    rig.shutdown();
}

// -----------------------------------------------------------------------
// Test 3: unknown_tool_name
// -----------------------------------------------------------------------

#[tokio::test]
async fn unknown_tool_name() {
    let (_trace, _responses, rig) = run_worker_spot_case(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/llm_traces/worker/unknown_tool.json"
        ),
        "Deploy to production",
    )
    .await
    .expect("failed to run worker spot case");

    // The deploy_to_production tool should have been attempted but failed.
    let completed = rig.tool_calls_completed();
    let deploy_results: Vec<_> = completed
        .iter()
        .filter(|(name, _)| name == "deploy_to_production")
        .collect();
    assert!(
        !deploy_results.is_empty(),
        "deploy_to_production should have been attempted: {completed:?}"
    );
    assert!(
        deploy_results.iter().all(|(_, ok)| !ok),
        "deploy_to_production should fail: {deploy_results:?}"
    );

    rig.shutdown();
}

// -----------------------------------------------------------------------
// Test 4: invalid_tool_params
// -----------------------------------------------------------------------

#[tokio::test]
async fn invalid_tool_params() {
    let (_trace, _responses, rig) = run_worker_spot_case(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/llm_traces/worker/invalid_params.json"
        ),
        "Echo something with wrong params first",
    )
    .await
    .expect("failed to run worker spot case");

    // Echo should have been called at least twice (bad then good).
    let started = rig.tool_calls_started();
    let echo_count = started.iter().filter(|n| n.as_str() == "echo").count();
    assert!(
        echo_count >= 2,
        "Expected >= 2 echo calls, got {echo_count}"
    );

    rig.shutdown();
}

// -----------------------------------------------------------------------
// Test 5: rate_limit_cascade
// -----------------------------------------------------------------------

#[tokio::test]
async fn rate_limit_cascade() {
    let trace = LlmTrace::from_file_async(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/worker/rate_limit_cascade.json"
    ))
    .await
    .expect("failed to load rate_limit_cascade.json");

    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .with_extra_tools(vec![Arc::new(StubRateLimitTool) as Arc<dyn Tool>])
        .build()
        .await
        .expect("failed to build test rig");

    rig.send_message("Call the rate limited tool").await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(15)).await;

    rig.verify_trace_expects(&trace, &responses);

    // Both calls should have failed due to rate limiting.
    let completed = rig.tool_calls_completed();
    let rl_calls: Vec<_> = completed
        .iter()
        .filter(|(name, _)| name == "stub_rate_limit")
        .collect();
    assert!(
        !rl_calls.is_empty(),
        "Expected stub_rate_limit calls: {completed:?}"
    );
    assert!(
        rl_calls.iter().all(|(_, ok)| !ok),
        "All stub_rate_limit calls should fail: {rl_calls:?}"
    );

    rig.shutdown();
}

// -----------------------------------------------------------------------
// Test 6: iteration_limit
// -----------------------------------------------------------------------

#[tokio::test]
async fn iteration_limit() {
    let trace = LlmTrace::from_file_async(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/worker/worker_timeout.json"
    ))
    .await
    .expect("failed to load worker_timeout.json");

    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .with_max_tool_iterations(2)
        .build()
        .await
        .expect("failed to build test rig");

    rig.send_message("Keep calling tools until the limit").await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(15)).await;

    // We should still get a response even with iteration limit.
    assert!(
        !responses.is_empty(),
        "Expected at least one response with iteration limit"
    );

    rig.verify_trace_expects(&trace, &responses);

    // Metrics should show we hit the iteration limit.
    let metrics = rig.collect_metrics().await;
    assert!(
        metrics.hit_iteration_limit,
        "Expected iteration limit stop, got metrics: {metrics:?}"
    );
    assert!(
        metrics.tool_calls.len() <= 2,
        "Expected at most 2 tool calls with limit=2, got {}",
        metrics.tool_calls.len()
    );

    rig.shutdown();
}

// -----------------------------------------------------------------------
// Test 7: simple_echo_flow
// -----------------------------------------------------------------------

#[tokio::test]
async fn simple_echo_flow() {
    let (_trace, _responses, rig) = run_worker_spot_case(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/llm_traces/worker/plan_remaining_work.json"
        ),
        "Plan and execute a task",
    )
    .await
    .expect("failed to run worker spot case");

    // Verify echo was called during execution.
    let started = rig.tool_calls_started();
    assert!(
        started.contains(&"echo".to_string()),
        "echo should be called: {started:?}"
    );

    rig.shutdown();
}
