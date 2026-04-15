//! Time tool tests: parse, diff, and invalid timestamp handling.

use super::common::{RigConfig, run_trace_test};

#[tokio::test]
async fn time_parse_and_diff() -> anyhow::Result<()> {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/tools/time_parse_diff.json"
    );
    let (rig, _trace, responses) = run_trace_test(
        fixture_path,
        "Parse a time and compute a diff",
        RigConfig {
            auto_approve: true,
            routines: false,
            skills: true,
        },
    )
    .await?;

    let result: anyhow::Result<()> = (|| {
        // Time tool should have been called twice (parse + diff).
        let started = rig.tool_calls_started();
        let time_count = started.iter().filter(|n| n.as_str() == "time").count();
        assert!(
            time_count >= 2,
            "Expected >= 2 time tool calls, got {time_count}"
        );
        let time_results: Vec<_> = rig
            .tool_results()
            .into_iter()
            .filter(|(name, _)| name == "time")
            .collect();
        assert_eq!(time_results.len(), 2, "expected exactly 2 time results");
        insta::assert_snapshot!("time_parse_result", time_results[0].1);
        insta::assert_snapshot!("time_diff_result", time_results[1].1);
        insta::assert_snapshot!("time_parse_and_diff_response", responses[0].content);
        Ok(())
    })();
    rig.shutdown();
    result
}

#[tokio::test]
async fn time_parse_invalid() -> anyhow::Result<()> {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/tools/time_parse_invalid.json"
    );
    let (rig, _trace, responses) = run_trace_test(
        fixture_path,
        "Parse an invalid timestamp",
        RigConfig {
            auto_approve: true,
            routines: false,
            skills: true,
        },
    )
    .await?;

    let result: anyhow::Result<()> = (|| {
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
        let time_result_previews: Vec<_> = rig
            .tool_results()
            .into_iter()
            .filter(|(name, _)| name == "time")
            .map(|(_, preview)| preview)
            .collect();
        insta::assert_snapshot!("time_parse_invalid_result", time_result_previews[0]);
        insta::assert_snapshot!("time_parse_invalid_response", responses[0].content);
        Ok(())
    })();
    rig.shutdown();
    result
}
