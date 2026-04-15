//! Routine tool tests: create, list, update, delete, history, and event emit.

use std::time::Duration;

use rstest::rstest;

use super::common::{RigConfig, run_trace_test, run_trace_test_with_timeout};

#[rstest]
#[case(
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/llm_traces/tools/routine_create_list.json"),
    "Create a daily routine and list all routines",
    &["routine_create", "routine_list"][..]
)]
#[case(
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/llm_traces/tools/routine_update_delete.json"),
    "Create, update, and delete a routine",
    &["routine_create", "routine_update", "routine_delete"][..]
)]
#[case(
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/llm_traces/tools/routine_history.json"),
    "Create a routine and check its history",
    &["routine_create", "routine_history"][..]
)]
#[tokio::test]
async fn routine_tools(
    #[case] fixture_path: &str,
    #[case] message: &str,
    #[case] expected_tools: &[&str],
) -> anyhow::Result<()> {
    let (rig, _trace, _responses) = run_trace_test(
        fixture_path,
        message,
        RigConfig {
            auto_approve: true,
            routines: true,
            skills: true,
        },
    )
    .await?;

    // Verify all expected tools completed successfully.
    let completed = rig.tool_calls_completed();
    for tool in expected_tools {
        assert!(
            completed.iter().any(|(n, ok)| n == tool && *ok),
            "{tool} should succeed: {completed:?}"
        );
    }

    rig.shutdown();
    Ok(())
}

#[tokio::test]
async fn routine_system_event_emit() -> anyhow::Result<()> {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/tools/routine_system_event_emit.json"
    );
    let (rig, _trace, _responses) = run_trace_test(
        fixture_path,
        "Create a system-event routine and emit an event",
        RigConfig {
            auto_approve: true,
            routines: true,
            skills: false,
        },
    )
    .await?;

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
    let emit_json: serde_json::Value =
        serde_json::from_str(&emit_result.1).expect("event_emit result should be valid JSON");
    insta::assert_json_snapshot!("routine_system_event_emit_payload", emit_json);

    rig.shutdown();
    Ok(())
}

#[tokio::test]
async fn skill_install_routine_webhook_sim() -> anyhow::Result<()> {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/tools/skill_install_routine_webhook_sim.json"
    );
    let (rig, _trace, _responses) = run_trace_test_with_timeout(
        fixture_path,
        "Install the workflow skill template and simulate a webhook routine run",
        RigConfig {
            auto_approve: true,
            routines: true,
            skills: true,
        },
        Duration::from_secs(20),
    )
    .await?;

    let completed = rig.tool_calls_completed();
    assert!(
        completed.iter().any(|(n, ok)| n == "skill_install" && *ok),
        "skill_install should succeed: {completed:?}"
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
    let emit_payload: serde_json::Value =
        serde_json::from_str(&emit_result.1).expect("event_emit result should be valid JSON");
    insta::assert_json_snapshot!("skill_install_emit_payload", emit_payload);

    let history_result = results
        .iter()
        .find(|(n, _)| n == "routine_history")
        .expect("routine_history result missing");
    let history_json: serde_json::Value = serde_json::from_str(&history_result.1)
        .expect("routine_history result should be valid JSON");
    insta::assert_json_snapshot!("skill_install_routine_history_payload", history_json);

    rig.shutdown();
    Ok(())
}
