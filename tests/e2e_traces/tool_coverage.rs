//! E2E trace tests: tool coverage.
//!
//! Exercises tools that were previously untested: json, shell, list_dir,
//! apply_patch, memory_read, and memory_tree.

use std::time::Duration;

use anyhow::Context as _;
use ironclaw::channels::OutgoingResponse;

use crate::support::cleanup::{CleanupGuard, setup_test_dir_with_suffix};
use crate::support::test_rig::{TestRig, TestRigBuilder};
use crate::support::trace_llm::LlmTrace;

fn test_dir_base() -> std::path::PathBuf {
    std::env::temp_dir().join("ironclaw_coverage_test")
}

async fn run_trace(
    fixture_path: &str,
    message: &str,
    path_replacements: &[(&str, &str)],
) -> (LlmTrace, Vec<OutgoingResponse>, TestRig) {
    let mut trace = LlmTrace::from_file_async(fixture_path)
        .await
        .with_context(|| format!("failed to load {fixture_path}"))
        .expect("failed to load trace fixture");
    for (from, to) in path_replacements {
        trace.patch_path(from, to);
    }
    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .build()
        .await
        .expect("failed to build test rig");
    rig.send_message(message).await;
    let responses = rig.wait_for_responses(1, Duration::from_secs(15)).await;
    (trace, responses, rig)
}

// -----------------------------------------------------------------------
// json tool
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_json_operations() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/coverage/json_operations.json"
    );
    let (trace, responses, rig) =
        run_trace(fixture_path, "Parse and query this json data", &[]).await;

    rig.verify_trace_expects(&trace, &responses);

    // Extra: verify json tool was called at least 3 times.
    let started = rig.tool_calls_started();
    assert!(
        started.iter().filter(|n| n.as_str() == "json").count() >= 3,
        "Expected at least 3 json tool calls, got: {:?}",
        started
    );

    // Extra: metrics checks.
    let metrics = rig.collect_metrics().await;
    assert!(
        metrics.llm_calls >= 4,
        "Expected >= 4 LLM calls, got {}",
        metrics.llm_calls
    );

    rig.shutdown();
}

// -----------------------------------------------------------------------
// shell tool
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_shell_echo() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/coverage/shell_echo.json"
    );
    let (trace, responses, rig) = run_trace(fixture_path, "Run a shell command for me", &[]).await;

    rig.verify_trace_expects(&trace, &responses);
    rig.shutdown();
}

// -----------------------------------------------------------------------
// list_dir tool
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_list_dir() {
    let test_dir = setup_test_dir_with_suffix(&test_dir_base(), "list_dir")
        .expect("failed to create list_dir test directory");
    let _cleanup = CleanupGuard::new().dir(&test_dir);
    tokio::fs::write(format!("{test_dir}/file_a.txt"), "content a")
        .await
        .expect("failed writing file_a.txt");
    tokio::fs::write(format!("{test_dir}/file_b.txt"), "content b")
        .await
        .expect("failed writing file_b.txt");

    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/coverage/list_dir.json"
    );
    let (trace, responses, rig) = run_trace(
        fixture_path,
        "List the test directory",
        &[("/tmp/ironclaw_coverage_test_list_dir", test_dir.as_str())],
    )
    .await;

    rig.verify_trace_expects(&trace, &responses);
    rig.shutdown();
}

// -----------------------------------------------------------------------
// apply_patch tool
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_apply_patch_chain() {
    let test_dir = setup_test_dir_with_suffix(&test_dir_base(), "apply_patch")
        .expect("failed to create apply_patch test directory");
    let _cleanup = CleanupGuard::new().dir(&test_dir);

    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/coverage/apply_patch_chain.json"
    );
    let (trace, responses, rig) = run_trace(
        fixture_path,
        "Write a file and patch it",
        &[("/tmp/ironclaw_coverage_test_apply_patch", test_dir.as_str())],
    )
    .await;

    rig.verify_trace_expects(&trace, &responses);

    // Extra: verify the patch was applied on disk.
    let content = tokio::fs::read_to_string(format!("{test_dir}/patch_target.txt"))
        .await
        .expect("failed reading patch_target.txt");
    assert!(
        content.contains("PATCHED"),
        "Expected 'PATCHED' in file content, got: {content:?}"
    );
    assert!(
        !content.contains("original"),
        "Expected 'original' to be replaced, but it still exists in: {content:?}"
    );

    // Extra: metrics checks.
    let metrics = rig.collect_metrics().await;
    assert!(metrics.llm_calls >= 4, "Expected >= 4 LLM calls");
    assert!(metrics.total_tool_calls() >= 3, "Expected >= 3 tool calls");

    rig.shutdown();
}

// -----------------------------------------------------------------------
// memory_read + memory_tree (full memory cycle)
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_memory_full_cycle() {
    let fixture_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/llm_traces/coverage/memory_full_cycle.json"
    );
    let (trace, responses, rig) =
        run_trace(fixture_path, "Exercise all four memory operations", &[]).await;

    rig.verify_trace_expects(&trace, &responses);

    // Extra: metrics checks.
    let metrics = rig.collect_metrics().await;
    assert!(metrics.llm_calls >= 5, "Expected >= 5 LLM calls");
    assert!(metrics.total_tool_calls() >= 4, "Expected >= 4 tool calls");

    rig.shutdown();
}
