//! Routine tool tests: create, list, update, delete, history, and event emit.

use super::common::{
    RigConfig, run_routine_started_test, run_trace_test, run_trace_test_with_timeout,
};
use crate::builtin_tool_coverage::common::routine_started_test;

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
            routines: true,
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

// Invoke the macro for routine tests that verify specific tools were started.
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
            routines: true,
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
    use std::time::Duration;

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
    .await;

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
    let fired_routines = emit_payload
        .get("fired_routines")
        .and_then(serde_json::Value::as_i64)
        .expect("event_emit result should include integer fired_routines");
    assert!(
        fired_routines > 0,
        "event_emit should report fired routines > 0: {emit_payload:?}"
    );

    let _history_result = results
        .iter()
        .find(|(n, _)| n == "routine_history")
        .expect("routine_history result missing");

    rig.shutdown();
}
