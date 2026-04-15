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
    Ok(())
}
