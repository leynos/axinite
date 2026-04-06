//! HTTP tool tests: GET requests with replay.

use super::common::{RigConfig, run_trace_test};

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
