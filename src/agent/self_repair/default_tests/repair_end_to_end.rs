//! End-to-end tests for default self-repair.

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
async fn repair_broken_tool_end_to_end_returns_success() {
    let cm = Arc::new(ContextManager::new(10));
    let store = Arc::new(CapturingStore::new());
    let store_for_repair: Arc<dyn crate::db::Database> = store.clone();

    let builder = Arc::new(StubSoftwareBuilder(StubBuilderOutcome::BuildSucceeded {
        is_success: true,
        error: None,
        iterations: 1,
        is_registered: false,
    }));
    let builder_for_repair: Arc<dyn crate::tools::SoftwareBuilder> = builder;
    let tools = Arc::new(crate::tools::ToolRegistry::new());

    let repair = DefaultSelfRepair::new(cm, Duration::from_secs(60), 3)
        .with_store(store_for_repair)
        .with_builder(builder_for_repair, tools);

    let broken = BrokenTool {
        name: "my-tool".to_string(),
        failure_count: 2,
        last_error: Some("test error".to_string()),
        first_failure: Utc::now(),
        last_failure: Utc::now(),
        last_build_result: None,
        repair_attempts: 0,
    };

    let result = NativeSelfRepair::repair_broken_tool(&repair, &broken)
        .await
        .expect("repair_broken_tool should not error");

    assert!(
        matches!(result, RepairResult::Success { .. }),
        "expected RepairResult::Success end-to-end, got: {result:?}",
    );
    assert_eq!(
        *store.calls().repaired_tools.lock().await,
        vec!["my-tool".to_string()],
        "repair_broken_tool should mark the tool repaired in the store",
    );
}

#[cfg(any(test, feature = "self_repair_extras"))]
#[tokio::test]
async fn repair_broken_tool_allows_one_concurrent_repair_for_same_tool() {
    let cm = Arc::new(ContextManager::new(10));
    let store = Arc::new(CapturingStore::new());
    let store_for_repair: Arc<dyn crate::db::Database> = store.clone();

    let builder = Arc::new(StubSoftwareBuilder(StubBuilderOutcome::BuildSucceeded {
        is_success: true,
        error: None,
        iterations: 1,
        is_registered: false,
    }));
    let builder_for_repair: Arc<dyn crate::tools::SoftwareBuilder> = builder;
    let tools = Arc::new(crate::tools::ToolRegistry::new());

    let repair = Arc::new(
        DefaultSelfRepair::new(cm, Duration::from_secs(60), 3)
            .with_store(store_for_repair)
            .with_builder(builder_for_repair, tools),
    );

    let broken = Arc::new(BrokenTool {
        name: "my-tool".to_string(),
        failure_count: 2,
        last_error: Some("test error".to_string()),
        first_failure: Utc::now(),
        last_failure: Utc::now(),
        last_build_result: None,
        repair_attempts: 0,
    });

    let first_repair = Arc::clone(&repair);
    let first_broken = Arc::clone(&broken);
    let first = tokio::spawn(async move {
        NativeSelfRepair::repair_broken_tool(first_repair.as_ref(), first_broken.as_ref()).await
    });
    let second_repair = Arc::clone(&repair);
    let second_broken = Arc::clone(&broken);
    let second = tokio::spawn(async move {
        NativeSelfRepair::repair_broken_tool(second_repair.as_ref(), second_broken.as_ref()).await
    });

    let (first, second) = tokio::join!(
        async { first.await.expect("first repair task should complete") },
        async { second.await.expect("second repair task should complete") },
    );
    let results = [
        first.expect("first repair_broken_tool call should not error"),
        second.expect("second repair_broken_tool call should not error"),
    ];

    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, RepairResult::Success { .. }))
            .count(),
        1,
        "exactly one concurrent repair should succeed",
    );
    assert!(
        results.iter().any(|result| matches!(
            result,
            RepairResult::Retry { message }
                if message == "Repair already in progress for 'my-tool'"
        )),
        "one concurrent repair should be rejected as already in progress: {results:?}",
    );
    assert_eq!(
        *store.calls().repaired_tools.lock().await,
        vec!["my-tool".to_string()],
        "only the claimed repair should mark the tool repaired",
    );
}
