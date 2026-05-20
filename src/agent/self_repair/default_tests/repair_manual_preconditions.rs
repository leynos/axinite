//! Manual precondition tests for default self-repair.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;

use crate::agent::self_repair::default::DefaultSelfRepair;
use crate::agent::self_repair::{BrokenTool, NativeSelfRepair, RepairResult};
use crate::context::ContextManager;
use crate::testing::null_db::CapturingStore;

use super::helpers::{StubBuilderOutcome, StubSoftwareBuilder};

#[cfg(any(test, feature = "self_repair_extras"))]
#[tokio::test]
async fn repair_broken_tool_returns_manual_without_store() {
    let cm = Arc::new(ContextManager::new(10));
    let builder = Arc::new(StubSoftwareBuilder::new(
        StubBuilderOutcome::BuildSucceeded {
            is_success: true,
            error: None,
            iterations: 1,
            is_registered: false,
        },
    ));
    let tools = Arc::new(crate::tools::ToolRegistry::new());
    let repair = DefaultSelfRepair::new(cm, Duration::from_secs(60), 3)
        .with_builder(builder as Arc<dyn crate::tools::SoftwareBuilder>, tools);

    let broken = BrokenTool {
        name: "my-tool".to_string(),
        failure_count: 1,
        last_error: None,
        first_failure: Utc::now(),
        last_failure: Utc::now(),
        last_build_result: None,
        repair_attempts: 0,
    };

    let result = NativeSelfRepair::repair_broken_tool(&repair, &broken)
        .await
        .expect("repair_broken_tool should not return Err in this precondition path");

    let RepairResult::ManualRequired { message } = result else {
        panic!("expected ManualRequired, got: {result:?}");
    };
    assert_eq!(message, "Store not available for tracking repair");
}

#[cfg(any(test, feature = "self_repair_extras"))]
#[tokio::test]
async fn repair_broken_tool_returns_manual_when_attempt_limit_exceeded() {
    let cm = Arc::new(ContextManager::new(10));
    let store = Arc::new(CapturingStore::new());
    let builder = Arc::new(StubSoftwareBuilder::new(
        StubBuilderOutcome::BuildSucceeded {
            is_success: true,
            error: None,
            iterations: 1,
            is_registered: false,
        },
    ));
    let tools = Arc::new(crate::tools::ToolRegistry::new());
    let repair = DefaultSelfRepair::new(cm, Duration::from_secs(60), 3)
        .with_store(store as Arc<dyn crate::db::Database>)
        .with_builder(builder as Arc<dyn crate::tools::SoftwareBuilder>, tools);

    // Input tool already at the limit; with Null/Capturing store there is no
    // persisted record, so the input value is used and should trigger
    // ManualRequired.
    let broken = BrokenTool {
        name: "my-tool".to_string(),
        failure_count: 1,
        last_error: None,
        first_failure: Utc::now(),
        last_failure: Utc::now(),
        last_build_result: None,
        repair_attempts: 3,
    };

    let result = NativeSelfRepair::repair_broken_tool(&repair, &broken)
        .await
        .expect("repair_broken_tool should not return Err here");

    let RepairResult::ManualRequired { message } = result else {
        panic!("expected ManualRequired, got: {result:?}");
    };
    assert_eq!(message, "Tool 'my-tool' exceeded max repair attempts (3)");
}
