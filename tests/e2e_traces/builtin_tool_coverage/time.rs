//! Time tool tests: parse, diff, and invalid timestamp handling.

use super::common::{run_trace_test, RigConfig};

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
            routines: false,
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
            routines: false,
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
