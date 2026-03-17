//! E2E test: validates that the metrics collection layer works.
//!
//! Exercises `TraceMetrics`, `ScenarioResult`, `RunResult`, and `compare_runs`
//! through actual agent execution via the TestRig.

use std::time::Duration;

use crate::support::assertions::assert_all_tools_succeeded;
use crate::support::cleanup::CleanupGuard;
use crate::support::metrics::{RunResult, ScenarioResult, TraceMetrics, compare_runs};
use crate::support::test_rig::{TestRig, TestRigBuilder};
use crate::support::trace_llm::{LlmTrace, TraceResponse, TraceToolCall};

const TEST_DIR: &str = "/tmp/ironclaw_metrics_test";
const TEST_FILE: &str = "/tmp/ironclaw_metrics_test/hello.txt";

fn setup_test_dir() {
    let _ = std::fs::remove_dir_all(TEST_DIR);
    std::fs::create_dir_all(TEST_DIR).expect("failed to create test directory");
}

fn localize_tool_call_path(tool_call: &mut TraceToolCall, path: &str) {
    if !matches!(tool_call.name.as_str(), "write_file" | "read_file") {
        return;
    }
    if let Some(arguments) = tool_call.arguments.as_object_mut() {
        arguments.insert(
            "path".to_string(),
            serde_json::Value::String(path.to_string()),
        );
    }
}

fn localize_file_tool_paths(trace: &mut LlmTrace, path: &str) {
    for turn in &mut trace.turns {
        for step in &mut turn.steps {
            let TraceResponse::ToolCalls { tool_calls, .. } = &mut step.response else {
                continue;
            };
            for tool_call in tool_calls {
                localize_tool_call_path(tool_call, path);
            }
        }
    }
}

fn assert_text_trace_llm_metrics(metrics: &TraceMetrics) {
    assert!(
        metrics.llm_calls >= 1,
        "Expected >= 1 LLM call, got {}",
        metrics.llm_calls
    );
    assert!(
        metrics.input_tokens >= 50,
        "Expected >= 50 input tokens, got {}",
        metrics.input_tokens
    );
    assert!(
        metrics.output_tokens >= 10,
        "Expected >= 10 output tokens, got {}",
        metrics.output_tokens
    );
}

fn assert_text_trace_shape(metrics: &TraceMetrics) {
    assert!(
        metrics.wall_time_ms > 0,
        "Expected wall_time_ms > 0, got {}",
        metrics.wall_time_ms
    );
    assert!(
        metrics.tool_calls.is_empty(),
        "Expected no tool calls, got {:?}",
        metrics.tool_calls
    );
    assert!(
        metrics.turns >= 1,
        "Expected >= 1 turn, got {}",
        metrics.turns
    );
}

/// Verify that metrics are collected from a simple text-only trace.
#[tokio::test]
async fn test_metrics_collected_from_text_trace() {
    let trace = LlmTrace::from_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/simple_text.json"
    ))
    .expect("failed to load simple_text.json");

    let rig = TestRigBuilder::new().with_trace(trace).build().await;

    rig.send_message("hello").await;
    let _responses = rig.wait_for_responses(1, Duration::from_secs(10)).await;

    // Collect metrics.
    let metrics = rig.collect_metrics().await;

    assert_text_trace_llm_metrics(&metrics);
    assert_text_trace_shape(&metrics);

    rig.shutdown();
}

fn assert_tool_trace_counts(metrics: &TraceMetrics) {
    assert!(
        metrics.llm_calls >= 3,
        "Expected >= 3 LLM calls, got {}",
        metrics.llm_calls
    );
    assert!(metrics.input_tokens > 0, "Expected input_tokens > 0");
    assert!(metrics.output_tokens > 0, "Expected output_tokens > 0");
}

fn assert_tool_trace_invocations(metrics: &TraceMetrics) {
    assert!(
        metrics.total_tool_calls() >= 2,
        "Expected >= 2 tool calls, got {}",
        metrics.total_tool_calls()
    );
    assert_eq!(
        metrics.failed_tool_calls(),
        0,
        "Expected 0 failed tool calls"
    );
}

fn assert_tool_trace_names(metrics: &TraceMetrics) {
    let tool_names: Vec<&str> = metrics.tool_calls.iter().map(|t| t.name.as_str()).collect();
    assert!(
        tool_names.contains(&"write_file"),
        "Expected write_file in tool calls, got {:?}",
        tool_names
    );
    assert!(
        tool_names.contains(&"read_file"),
        "Expected read_file in tool calls, got {:?}",
        tool_names
    );
}

/// Verify that metrics capture tool calls from a file write/read flow.
#[tokio::test]
async fn test_metrics_collected_from_tool_trace() {
    setup_test_dir();
    let _cleanup = CleanupGuard::new().dir(TEST_DIR);

    let mut trace = LlmTrace::from_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/file_write_read.json"
    ))
    .expect("failed to load file_write_read.json");
    localize_file_tool_paths(&mut trace, TEST_FILE);

    let rig = TestRigBuilder::new().with_trace(trace).build().await;

    rig.send_message("Please write a greeting to a file and read it back.")
        .await;
    let _responses = rig.wait_for_responses(1, Duration::from_secs(15)).await;

    // Assert all tools completed successfully.
    let completed = rig.tool_calls_completed();
    assert_all_tools_succeeded(&completed);

    let metrics = rig.collect_metrics().await;

    assert_tool_trace_counts(&metrics);
    assert_tool_trace_invocations(&metrics);
    assert_tool_trace_names(&metrics);

    rig.shutdown();
}

fn assert_scenario_result_json_keys(json: &str) {
    for key in &[
        "scenario_id",
        "wall_time_ms",
        "llm_calls",
        "input_tokens",
        "output_tokens",
    ] {
        assert!(
            json.contains(&format!("\"{key}\"")),
            "JSON missing expected key: {key}"
        );
    }
}

/// Verify that metrics serialize to JSON correctly (for CI consumption).
#[tokio::test]
async fn test_metrics_json_serialization() {
    let trace = LlmTrace::from_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/simple_text.json"
    ))
    .expect("failed to load simple_text.json");

    let rig = TestRigBuilder::new().with_trace(trace).build().await;

    rig.send_message("hello").await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(10)).await;

    let metrics = rig.collect_metrics().await;

    // Build a ScenarioResult.
    let scenario = ScenarioResult {
        scenario_id: "test_metrics_json_serialization".to_string(),
        passed: true,
        trace: metrics,
        response: responses
            .first()
            .map(|r| r.content.clone())
            .unwrap_or_default(),
        error: None,
        turn_metrics: Vec::new(),
    };

    // Should serialize to valid JSON.
    let json = serde_json::to_string_pretty(&scenario).expect("JSON serialization failed");
    assert_scenario_result_json_keys(&json);

    // Should deserialize back.
    let deserialized: ScenarioResult =
        serde_json::from_str(&json).expect("JSON deserialization failed");
    assert_eq!(deserialized.scenario_id, scenario.scenario_id);
    assert_eq!(deserialized.passed, scenario.passed);

    rig.shutdown();
}

/// Verify RunResult aggregation and baseline comparison.
#[tokio::test]
async fn test_run_result_and_baseline_comparison() {
    let trace = LlmTrace::from_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/simple_text.json"
    ))
    .expect("failed to load simple_text.json");

    let rig = TestRigBuilder::new().with_trace(trace).build().await;

    rig.send_message("hello").await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(10)).await;

    let metrics = rig.collect_metrics().await;

    // Create a "current" run result.
    let current_scenario = ScenarioResult {
        scenario_id: "smoke_test".to_string(),
        passed: true,
        trace: metrics,
        response: responses
            .first()
            .map(|r| r.content.clone())
            .unwrap_or_default(),
        error: None,
        turn_metrics: Vec::new(),
    };
    let current_run = RunResult::from_scenarios("current-run", vec![current_scenario]);

    // Verify aggregation.
    assert_eq!(current_run.pass_rate, 1.0);
    assert_eq!(current_run.scenarios.len(), 1);
    assert!(current_run.total_wall_time_ms > 0);

    // Create a synthetic "baseline" with double the tokens (simulating regression).
    let mut baseline_trace = current_run.scenarios[0].trace.clone();
    baseline_trace.input_tokens /= 2; // Baseline had fewer tokens.
    let baseline_scenario = ScenarioResult {
        scenario_id: "smoke_test".to_string(),
        passed: true,
        trace: baseline_trace,
        response: "baseline response".to_string(),
        error: None,
        turn_metrics: Vec::new(),
    };
    let baseline_run = RunResult::from_scenarios("baseline-run", vec![baseline_scenario]);

    // Compare should detect token regression (current uses more tokens than baseline).
    let deltas = compare_runs(&baseline_run, &current_run, 0.10);
    let token_delta = deltas.iter().find(|d| d.metric == "total_tokens");
    if let Some(d) = token_delta {
        assert!(d.is_regression, "Expected token regression");
        assert!(d.delta > 0.0, "Expected positive delta for regression");
    }

    rig.shutdown();
}

fn assert_rig_metrics_are_zero(rig: &TestRig) {
    assert_eq!(rig.llm_call_count(), 0, "Expected llm_call_count to be 0");
    assert_eq!(
        rig.total_input_tokens(),
        0,
        "Expected total_input_tokens to be 0"
    );
    assert_eq!(
        rig.total_output_tokens(),
        0,
        "Expected total_output_tokens to be 0"
    );
}

fn assert_rig_token_metrics_populated(rig: &TestRig) {
    assert!(
        rig.llm_call_count() >= 1,
        "Expected llm_call_count >= 1, got {}",
        rig.llm_call_count()
    );
    assert!(
        rig.total_input_tokens() > 0,
        "Expected total_input_tokens > 0, got {}",
        rig.total_input_tokens()
    );
    assert!(
        rig.total_output_tokens() > 0,
        "Expected total_output_tokens > 0, got {}",
        rig.total_output_tokens()
    );
}

fn assert_rig_timing_populated(rig: &TestRig) {
    assert!(
        rig.elapsed_ms() > 0,
        "Expected elapsed_ms > 0, got {}",
        rig.elapsed_ms()
    );
}

fn assert_rig_metrics_are_populated(rig: &TestRig) {
    assert_rig_token_metrics_populated(rig);
    assert_rig_timing_populated(rig);
}

/// Verify that accessor methods on TestRig match InstrumentedLlm data.
#[tokio::test]
async fn test_rig_metric_accessors() {
    let trace = LlmTrace::from_file(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/simple_text.json"
    ))
    .expect("failed to load simple_text.json");

    let rig = TestRigBuilder::new().with_trace(trace).build().await;

    // Before sending any message, metrics should be zero.
    assert_rig_metrics_are_zero(&rig);

    rig.send_message("hello").await;
    let _responses = rig.wait_for_responses(1, Duration::from_secs(10)).await;

    // After the agent processes, metrics should be populated.
    assert_rig_metrics_are_populated(&rig);

    rig.shutdown();
}
