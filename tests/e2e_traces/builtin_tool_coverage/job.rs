//! Job tool tests: create, status, list, and cancel operations.

use super::common::{RigConfig, run_trace_test};

// Uses {{call_cj_1.job_id}} template to forward the dynamic UUID from
// create_job's result into job_status's arguments.

#[tokio::test]
async fn job_create_status() -> anyhow::Result<()> {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/tools/job_create_status.json"
    );
    let (rig, _trace, _responses) = run_trace_test(
        fixture_path,
        "Create a job and check its status",
        RigConfig::default(),
    )
    .await?;
    let rig = scopeguard::guard(rig, |rig| rig.shutdown());

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
    let parsed_create = serde_json::from_str::<serde_json::Value>(&create_result.1)
        .expect("create_job result should be valid JSON");
    assert!(
        parsed_create
            .get("job_id")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|job_id| !job_id.is_empty()),
        "create_job should return a non-empty job_id: {parsed_create:?}"
    );
    assert_eq!(
        parsed_create.get("status").and_then(serde_json::Value::as_str),
        Some("in_progress"),
        "create_job should dispatch through the scheduler, not stay pending: {parsed_create:?}"
    );
    assert!(
        !parsed_create
            .get("error")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|error| error.contains("scheduler unavailable")),
        "create_job should not fall back to the unscheduled path: {parsed_create:?}"
    );
    let status_result = results
        .iter()
        .find(|(n, _)| n == "job_status")
        .expect("job_status result missing");
    let parsed_status = serde_json::from_str::<serde_json::Value>(&status_result.1)
        .expect("job_status result should be valid JSON");
    assert_eq!(
        parsed_status.get("title").and_then(serde_json::Value::as_str),
        Some("Test analysis job"),
        "job_status should return the job title: {parsed_status:?}"
    );
    Ok(())
}

// Uses {{call_cj_lc.job_id}} template to forward the dynamic UUID from
// create_job into cancel_job.

#[tokio::test]
async fn job_list_cancel() -> anyhow::Result<()> {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/tools/job_list_cancel.json"
    );
    let (rig, _trace, _responses) = run_trace_test(
        fixture_path,
        "Create a job, list jobs, then cancel it",
        RigConfig::default(),
    )
    .await?;
    let rig = scopeguard::guard(rig, |rig| rig.shutdown());

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
    let list_result = results
        .iter()
        .find(|(n, _)| n == "list_jobs")
        .expect("list_jobs result missing");
    assert!(
        !list_result.1.is_empty() && list_result.1.contains("job_id"),
        "list_jobs should return at least one job entry: {:?}",
        list_result.1
    );
    let cancel_result = results
        .iter()
        .find(|(n, _)| n == "cancel_job")
        .expect("cancel_job result missing");
    assert!(
        cancel_result.1.contains("cancel") || cancel_result.1.contains("cancelled"),
        "cancel_job should report a cancelled outcome: {:?}",
        cancel_result.1
    );
    Ok(())
}
