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

    assert!(
        matches!(repair, RepairResult::Success { .. }),
        "expected RepairResult::Success, got: {:?}",
        repair
    );
    if let RepairResult::Success { message } = repair {
        assert!(
            message.contains("my-tool"),
            "message should mention tool name"
        );
        assert!(
            message.contains('3'),
            "message should include iteration count"
        );
    }

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

    assert!(
        matches!(repair, RepairResult::Retry { .. }),
        "expected RepairResult::Retry, got: {:?}",
        repair
    );
    if let RepairResult::Retry { message } = repair {
        assert!(
            message.contains("compile error"),
            "message should include the build error"
        );
        assert!(
            message.contains("my-tool"),
            "message should mention tool name"
        );
        assert!(
            message.contains('2'),
            "message should include attempt number"
        );
    }
}

#[tokio::test]
async fn handle_build_result_uses_unknown_error_when_no_error_string() {
    let tool = stub_broken_tool("my-tool", None, 0);
    let result = stub_build_result(false, None, 1, false);
    let store = NullDatabase::new();

    let repair = DefaultSelfRepair::handle_build_result(result, &tool, &store)
        .await
        .expect("handle_build_result should not error");

    assert!(
        matches!(repair, RepairResult::Retry { .. }),
        "expected RepairResult::Retry, got: {:?}",
        repair
    );
    if let RepairResult::Retry { message } = repair {
        assert!(
            message.contains("Unknown error"),
            "message should say 'Unknown error' when error field is None"
        );
    }
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
