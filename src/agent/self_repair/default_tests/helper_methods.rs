//! Unit tests for DefaultSelfRepair helper methods.

use chrono::Utc;

use crate::agent::self_repair::BrokenTool;
use crate::agent::self_repair::default::DefaultSelfRepair;
use crate::error::RepairError;
use crate::testing::null_db::{CapturingStore, NullDatabase};
use crate::tools::BuildRequirement;

use super::helpers::{
    StubBuilderOutcome, StubSoftwareBuilder, stub_build_requirement, stub_build_result,
};

fn broken_tool(name: &str, attempts: u32) -> BrokenTool {
    BrokenTool {
        name: name.to_string(),
        failure_count: 1,
        last_error: None,
        first_failure: Utc::now(),
        last_failure: Utc::now(),
        last_build_result: None,
        repair_attempts: attempts,
    }
}

#[test]
fn build_repair_requirement_rejects_invalid_names() {
    for bad in ["", "bad name"] {
        let bt = broken_tool(bad, 0);
        let err =
            DefaultSelfRepair::build_repair_requirement(&bt).expect_err("invalid name must error");
        assert!(matches!(err, RepairError::Failed { .. }));
    }
}

#[tokio::test]
async fn handle_build_result_success_sets_message() {
    let bt = broken_tool("my-tool", 0);
    let db = NullDatabase::new();
    let result = stub_build_result(true, None, 2, false).expect("stub");
    let out = DefaultSelfRepair::handle_build_result(result, &bt, &db)
        .await
        .expect("ok");
    let crate::agent::self_repair::RepairResult::Success { message } = out else {
        panic!("expected Success");
    };
    assert_eq!(
        message,
        "Tool 'my-tool' repaired successfully after 2 iterations"
    );
}

#[tokio::test]
async fn handle_build_result_db_error_is_propagated() {
    let bt = broken_tool("my-tool", 0);
    let db =
        CapturingStore::failing_mark_tool_repaired_once(crate::error::DatabaseError::NotFound {
            entity: "tool_failure".into(),
            id: "simulated".into(),
        });
    let result = stub_build_result(true, None, 1, false).expect("stub");
    let err = DefaultSelfRepair::handle_build_result(result, &bt, &db)
        .await
        .expect_err("should error");
    assert!(matches!(err, RepairError::Failed { .. }));
}

#[tokio::test]
async fn attempt_repair_build_builder_error_becomes_retry() {
    let bt = broken_tool("my-tool", 0);
    let db = NullDatabase::new();
    let builder = StubSoftwareBuilder::new(StubBuilderOutcome::BuilderErrored("out of memory"));
    let req: BuildRequirement = stub_build_requirement().expect("stub");
    let out = DefaultSelfRepair::attempt_repair_build(&bt, &db, &builder, &req)
        .await
        .expect("ok");
    let crate::agent::self_repair::RepairResult::Retry { message } = out else {
        panic!("expected Retry");
    };
    assert_eq!(
        message,
        "Repair build error: Tool builder failed: out of memory"
    );
}
