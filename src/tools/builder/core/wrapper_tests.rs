//! Tests for the build software native-tool wrapper.

#[path = "wrapper_tests/execute.rs"]
mod execute;
#[path = "wrapper_tests/harness.rs"]
mod harness;

use std::sync::Arc;
use std::time::Duration;

use insta::assert_snapshot;

use super::clock::FixedMonotonicClock;
use super::*;
use harness::{FakeSoftwareBuilder, test_build_result, test_requirement};

#[tokio::test]
async fn execute_success_output_matches_snapshot() {
    let requirement = test_requirement().expect("test requirement should build");
    let build_result = test_build_result(requirement.clone());
    let builder = FakeSoftwareBuilder::success(requirement, build_result);
    let tool = BuildSoftwareTool::new_with_clock(
        Arc::new(builder),
        Arc::new(FixedMonotonicClock::with_elapsed(Duration::from_millis(42))),
    );

    let output = tool
        .execute(
            serde_json::json!({
                "description": "build a test tool",
            }),
            &JobContext::default(),
        )
        .await
        .expect("expected execute to return successful output");

    assert_eq!(
        output.duration,
        Duration::from_millis(42),
        "duration must reflect the clock seam, not wall time"
    );
    assert_eq!(output.cost, None);
    assert_eq!(output.raw, None);
    assert_snapshot!(
        serde_json::to_string_pretty(&output.result).expect("output result should serialize")
    );
}
