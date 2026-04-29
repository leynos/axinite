//! Tests for repair build result handling.

use crate::agent::self_repair::RepairResult;
use crate::agent::self_repair::default::DefaultSelfRepair;
use crate::testing::null_db::{CapturingStore, NullDatabase};

use super::helpers::{
    FailingRepairStore, failing_repair_store, stub_broken_tool, stub_build_result,
};

// === handle_build_result ===

#[tokio::test]
async fn handle_build_result_returns_success_when_build_succeeded() {
    let tool = stub_broken_tool("my-tool", None, 0);
    let result = stub_build_result(true, None, 3, false);
    let store = CapturingStore::new();

    let repair = DefaultSelfRepair::handle_build_result(result, &tool, &store)
        .await
        .expect("handle_build_result should not error");

    let RepairResult::Success { message } = repair else {
        panic!("expected RepairResult::Success");
    };
    assert_eq!(
        message,
        "Tool 'my-tool' repaired successfully after 3 iterations"
    );

    assert_eq!(
        *store.calls().repaired_tools.lock().await,
        vec!["my-tool".to_string()],
        "successful repair should mark the tool as repaired"
    );
}

#[tokio::test]
async fn handle_build_result_returns_retry_when_build_failed_with_error() {
    let tool = stub_broken_tool("my-tool", None, 1);
    let result = stub_build_result(false, Some("compile error"), 2, false);
    let store = NullDatabase::new();

    let repair = DefaultSelfRepair::handle_build_result(result, &tool, &store)
        .await
        .expect("handle_build_result should not error");

    let RepairResult::Retry { message } = repair else {
        panic!("expected RepairResult::Retry");
    };
    assert_eq!(
        message,
        "Repair attempt 2 for 'my-tool' failed: compile error"
    );
}

#[tokio::test]
async fn handle_build_result_uses_unknown_error_when_no_error_string() {
    let tool = stub_broken_tool("my-tool", None, 0);
    let result = stub_build_result(false, None, 1, false);
    let store = NullDatabase::new();

    let repair = DefaultSelfRepair::handle_build_result(result, &tool, &store)
        .await
        .expect("handle_build_result should not error");

    let RepairResult::Retry { message } = repair else {
        panic!("expected RepairResult::Retry");
    };
    assert_eq!(
        message,
        "Repair attempt 1 for 'my-tool' failed: Unknown error"
    );
}

#[tokio::test]
async fn handle_build_result_returns_error_when_mark_repaired_fails() {
    let tool = stub_broken_tool("my-tool", None, 0);
    let result = stub_build_result(true, None, 1, false);
    let store: FailingRepairStore = failing_repair_store();

    let err = DefaultSelfRepair::handle_build_result(result, &tool, &store)
        .await
        .expect_err("should propagate database error as RepairError");

    assert!(
        matches!(err, crate::error::RepairError::Failed { .. }),
        "expected RepairError::Failed when mark_tool_repaired errors, got: {err:?}",
    );
}

#[tokio::test]
async fn handle_build_result_records_each_call_independently() {
    // Calling handle_build_result twice for the same tool records two
    // mark_tool_repaired entries: there is no deduplication in the helper.
    // Deduplication is the responsibility of the database or the scheduler.
    let tool = stub_broken_tool("my-tool", None, 0);
    let store = CapturingStore::new();

    let result_a = stub_build_result(true, None, 1, false);
    DefaultSelfRepair::handle_build_result(result_a, &tool, &store)
        .await
        .expect("first call should succeed");

    let result_b = stub_build_result(true, None, 2, false);
    DefaultSelfRepair::handle_build_result(result_b, &tool, &store)
        .await
        .expect("second call should succeed");

    assert_eq!(
        *store.calls().repaired_tools.lock().await,
        vec!["my-tool".to_string(), "my-tool".to_string()],
        "each successful build call records mark_tool_repaired independently"
    );
}
