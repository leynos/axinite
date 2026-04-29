//! Tests for repair build attempts.

use rstest::rstest;

use crate::agent::self_repair::RepairResult;
use crate::agent::self_repair::default::DefaultSelfRepair;
use crate::testing::null_db::{CapturingStore, NullDatabase};

use super::helpers::stub_build_requirement;
use super::helpers::{StubBuilderOutcome, StubSoftwareBuilder, stub_broken_tool};

// === attempt_repair_build ===

#[rstest]
#[case(true, None, 2, true, true)]
#[case(false, Some("linker error"), 4, false, false)]
#[tokio::test]
async fn attempt_repair_build_propagates_build_outcome(
    #[case] is_success: bool,
    #[case] error: Option<&'static str>,
    #[case] iterations: u32,
    #[case] is_registered: bool,
    #[case] expect_success: bool,
) {
    let tool = stub_broken_tool("my-tool", None, 0);
    let store = CapturingStore::new();
    let builder = StubSoftwareBuilder(StubBuilderOutcome::BuildSucceeded {
        is_success,
        error,
        iterations,
        is_registered,
    });
    let requirement = stub_build_requirement();

    let repair = DefaultSelfRepair::attempt_repair_build(&tool, &store, &builder, &requirement)
        .await
        .expect("attempt_repair_build should not error");

    if expect_success {
        let RepairResult::Success { message } = repair else {
            panic!("expected RepairResult::Success, got: {repair:?}");
        };
        assert_eq!(
            message,
            "Tool 'my-tool' repaired successfully after 2 iterations"
        );
        assert_eq!(
            *store.calls().repaired_tools.lock().await,
            vec!["my-tool".to_string()],
            "successful build should mark tool as repaired"
        );
    } else {
        let RepairResult::Retry { message } = repair else {
            panic!("expected RepairResult::Retry, got: {repair:?}");
        };
        assert_eq!(
            message,
            "Repair attempt 1 for 'my-tool' failed: linker error"
        );
        assert!(
            store.calls().repaired_tools.lock().await.is_empty(),
            "failed build should not mark the tool as repaired"
        );
    }
}

#[tokio::test]
async fn attempt_repair_build_returns_retry_when_builder_itself_errors() {
    let tool = stub_broken_tool("my-tool", None, 0);
    let store = NullDatabase::new();
    let builder = StubSoftwareBuilder(StubBuilderOutcome::BuilderErrored("out of memory"));
    let requirement = stub_build_requirement();

    let repair = DefaultSelfRepair::attempt_repair_build(&tool, &store, &builder, &requirement)
        .await
        .expect("attempt_repair_build should propagate builder errors as Retry");

    let RepairResult::Retry { message } = repair else {
        panic!("expected RepairResult::Retry");
    };
    assert_eq!(
        message,
        "Repair build error: Tool builder failed: out of memory"
    );
}
