//! E2E trace tests: builtin tool coverage (#573).
//!
//! Covers time (parse, diff, invalid), routine (create, list, update, delete,
//! history), job (create, status, list, cancel), and HTTP replay.

use std::time::Duration;

use ironclaw::channels::OutgoingResponse;

use crate::support::test_rig::{TestRig, TestRigBuilder};
use crate::support::trace_llm::LlmTrace;

#[tokio::test]
async fn time_parse_and_diff() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/tools/time_parse_diff.json"
    );
    let (rig, _trace, _responses) = run_trace_test(
        fixture_path,
        "Parse a time and compute a diff",
        RigConfig {
            auto_approve: true,
            skills: true,
        },
    )
    .await;

    // Time tool should have been called twice (parse + diff).
    let started = rig.tool_calls_started();
    let time_count = started.iter().filter(|n| n.as_str() == "time").count();
    assert!(
        time_count >= 2,
        "Expected >= 2 time tool calls, got {time_count}"
    );

    rig.shutdown();
}

#[tokio::test]
async fn time_parse_invalid() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/tools/time_parse_invalid.json"
    );
    let (rig, _trace, _responses) = run_trace_test(
        fixture_path,
        "Parse an invalid timestamp",
        RigConfig {
            auto_approve: true,
            skills: true,
        },
    )
    .await;

    // The time tool call should have failed (invalid timestamp).
    let completed = rig.tool_calls_completed();
    let time_results: Vec<_> = completed
        .iter()
        .filter(|(name, _)| name == "time")
        .collect();
    assert!(!time_results.is_empty(), "Expected time tool to be called");
    assert!(
        time_results.iter().any(|(_, ok)| !ok),
        "Expected at least one failed time call: {time_results:?}"
    );

    rig.shutdown();
}

#[tokio::test]
async fn routine_create_list() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/tools/routine_create_list.json"
    );
    let (rig, _trace, _responses) = run_trace_test(
        fixture_path,
        "Create a daily routine and list all routines",
        RigConfig {
            auto_approve: true,
            skills: true,
        },
    )
    .await;

    // Both routine_create and routine_list should have succeeded.
    let completed = rig.tool_calls_completed();
    assert!(
        completed.iter().any(|(n, ok)| n == "routine_create" && *ok),
        "routine_create should succeed: {completed:?}"
    );
    assert!(
        completed.iter().any(|(n, ok)| n == "routine_list" && *ok),
        "routine_list should succeed: {completed:?}"
    );

    rig.shutdown();
}

#[derive(Default)]
struct RigConfig {
    auto_approve: bool,
    skills: bool,
}

async fn run_trace_test(
    fixture_path: &str,
    message: &str,
    config: RigConfig,
) -> (TestRig, LlmTrace, Vec<OutgoingResponse>) {
    run_trace_test_with_timeout(fixture_path, message, config, Duration::from_secs(15)).await
}

async fn run_trace_test_with_timeout(
    fixture_path: &str,
    message: &str,
    config: RigConfig,
    timeout: Duration,
) -> (TestRig, LlmTrace, Vec<OutgoingResponse>) {
    let trace = LlmTrace::from_file_async(fixture_path)
        .await
        .unwrap_or_else(|_| panic!("failed to load {fixture_path}"));

    let mut builder = TestRigBuilder::new().with_trace(trace.clone());
    if config.auto_approve {
        builder = builder.with_auto_approve_tools(true);
    }
    if config.skills {
        builder = builder.with_skills();
    }
    let rig = builder.build().await;

    rig.send_message(message).await;
    let responses = rig.wait_for_responses(1, timeout).await;

    rig.verify_trace_expects(&trace, &responses);
    (rig, trace, responses)
}

async fn run_routine_started_test(fixture_path: &str, message: &str, expected_tools: &[&str]) {
    let (rig, _trace, _responses) =
        run_trace_test(fixture_path, message, RigConfig::default()).await;
    let started = rig.tool_calls_started();
    for tool in expected_tools {
        assert!(
            started.contains(&(*tool).to_string()),
            "{tool} not started: {started:?}"
        );
    }

    rig.shutdown();
}

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

routine_started_test!(
    routine_update_delete,
    "/tests/fixtures/llm_traces/tools/routine_update_delete.json",
    "Create, update, and delete a routine",
    ["routine_create", "routine_update", "routine_delete"]
);

routine_started_test!(
    routine_history,
    "/tests/fixtures/llm_traces/tools/routine_history.json",
    "Create a routine and check its history",
    ["routine_create", "routine_history"]
);

#[tokio::test]
async fn routine_system_event_emit() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/tools/routine_system_event_emit.json"
    );
    let (rig, _trace, _responses) = run_trace_test(
        fixture_path,
        "Create a system-event routine and emit an event",
        RigConfig {
            auto_approve: true,
            skills: false,
        },
    )
    .await;

    let completed = rig.tool_calls_completed();
    assert!(
        completed.iter().any(|(n, ok)| n == "event_emit" && *ok),
        "event_emit should succeed: {completed:?}"
    );

    let results = rig.tool_results();
    let emit_result = results
        .iter()
        .find(|(n, _)| n == "event_emit")
        .expect("event_emit result missing");
    assert!(
        emit_result.1.contains("fired_routines"),
        "event_emit should report fired routine count: {:?}",
        emit_result.1
    );
    // Verify at least one routine actually fired (not just that the key exists).
    let emit_json: serde_json::Value =
        serde_json::from_str(&emit_result.1).expect("event_emit result should be valid JSON");
    assert!(
        emit_json["fired_routines"].as_u64().unwrap_or(0) > 0,
        "event_emit should have fired at least one routine: {:?}",
        emit_result.1
    );

    rig.shutdown();
}

#[tokio::test]
async fn skill_install_routine_webhook_sim() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/tools/skill_install_routine_webhook_sim.json"
    );
    let (rig, _trace, _responses) = run_trace_test_with_timeout(
        fixture_path,
        "Install the workflow skill template and simulate a webhook routine run",
        RigConfig {
            auto_approve: true,
            skills: true,
        },
        Duration::from_secs(20),
    )
    .await;

    let completed = rig.tool_calls_completed();
    assert!(
        completed.iter().any(|(n, _)| n == "skill_install"),
        "skill_install should be called: {completed:?}"
    );
    for tool in &["routine_create", "event_emit", "routine_history"] {
        assert!(
            completed.iter().any(|(n, ok)| n == tool && *ok),
            "{tool} should succeed: {completed:?}"
        );
    }

    let results = rig.tool_results();
    let emit_result = results
        .iter()
        .find(|(n, _)| n == "event_emit")
        .expect("event_emit result missing");
    assert!(
        emit_result.1.contains("fired_routines"),
        "event_emit should include fired_routines: {:?}",
        emit_result.1
    );

    let _history_result = results
        .iter()
        .find(|(n, _)| n == "routine_history")
        .expect("routine_history result missing");

    rig.shutdown();
}

// Uses {{call_cj_1.job_id}} template to forward the dynamic UUID from
// create_job's result into job_status's arguments.

#[tokio::test]
async fn job_create_status() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/tools/job_create_status.json"
    );
    let (rig, _trace, _responses) = run_trace_test(
        fixture_path,
        "Create a job and check its status",
        RigConfig::default(),
    )
    .await;

    // Both tools should have succeeded.
    let completed = rig.tool_calls_completed();
    assert!(
        completed.iter().any(|(n, ok)| n == "create_job" && *ok),
        "create_job should succeed: {completed:?}"
    );
    assert!(
        completed.iter().any(|(n, ok)| n == "job_status" && *ok),
        "job_status should succeed: {completed:?}"
    );

    // Verify tool results contain expected content.
    let results = rig.tool_results();
    let create_result = results
        .iter()
        .find(|(n, _)| n == "create_job")
        .expect("create_job result missing");
    assert!(
        create_result.1.contains("job_id"),
        "create_job should return a job_id: {:?}",
        create_result.1
    );
    assert!(
        create_result.1.contains("in_progress"),
        "create_job should dispatch through the scheduler, not stay pending: {:?}",
        create_result.1
    );
    assert!(
        !create_result.1.contains("scheduler unavailable"),
        "create_job should not fall back to the unscheduled path: {:?}",
        create_result.1
    );
    let status_result = results
        .iter()
        .find(|(n, _)| n == "job_status")
        .expect("job_status result missing");
    assert!(
        status_result.1.contains("Test analysis job"),
        "job_status should return the job title: {:?}",
        status_result.1
    );

    rig.shutdown();
}

// Uses {{call_cj_lc.job_id}} template to forward the dynamic UUID from
// create_job into cancel_job.

#[tokio::test]
async fn job_list_cancel() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/tools/job_list_cancel.json"
    );
    let (rig, _trace, _responses) = run_trace_test(
        fixture_path,
        "Create a job, list jobs, then cancel it",
        RigConfig::default(),
    )
    .await;

    // All three tools should have succeeded.
    let completed = rig.tool_calls_completed();
    assert!(
        completed.iter().any(|(n, ok)| n == "create_job" && *ok),
        "create_job should succeed: {completed:?}"
    );
    assert!(
        completed.iter().any(|(n, ok)| n == "list_jobs" && *ok),
        "list_jobs should succeed: {completed:?}"
    );
    assert!(
        completed.iter().any(|(n, ok)| n == "cancel_job" && *ok),
        "cancel_job should succeed: {completed:?}"
    );

    rig.shutdown();
}

#[tokio::test]
async fn http_get_with_replay() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/tools/http_get_replay.json"
    );
    let (rig, _trace, _responses) = run_trace_test(
        fixture_path,
        "Make an http GET request",
        RigConfig::default(),
    )
    .await;

    // HTTP tool should have succeeded with the replayed exchange.
    let completed = rig.tool_calls_completed();
    assert!(
        completed.iter().any(|(n, ok)| n == "http" && *ok),
        "http tool should succeed: {completed:?}"
    );

    rig.shutdown();
}
