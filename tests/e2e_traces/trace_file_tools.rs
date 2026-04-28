//! E2E trace test: validates that the agent can execute `write_file` and
//! `read_file` tool calls driven by a TraceLlm trace.

use std::time::Duration;

use crate::support::test_rig::TestRigBuilder;
use crate::support::trace_types::LlmTrace;

const EXPECTED_CONTENT: &str = "Hello, E2E test!";

#[tokio::test]
async fn test_file_write_and_read_flow() {
    let temp_dir = tempfile::tempdir().expect("failed to create trace file tools tempdir");
    let test_file = temp_dir.path().join("hello.txt");

    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/file_write_read.json"
    );
    let mut trace = LlmTrace::from_file_async(fixture_path)
        .await
        .expect("failed to load trace fixture");
    let patch_count = trace.patch_path(
        "/tmp/ironclaw_e2e_test/hello.txt",
        test_file
            .to_str()
            .expect("temp file path should be valid UTF-8"),
    );
    assert!(
        patch_count > 0,
        "expected to patch file_write_read.json test path"
    );

    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .build()
        .await
        .expect("failed to build test rig");

    rig.send_message("Please write a greeting to a file and read it back.")
        .await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(15)).await;

    rig.verify_trace_expects(&trace, &responses);

    // Extra: verify file on disk (can't express in expects).
    let file_content =
        std::fs::read_to_string(&test_file).expect("hello.txt should exist after write_file");
    assert_eq!(file_content, EXPECTED_CONTENT);

    rig.shutdown();
}
