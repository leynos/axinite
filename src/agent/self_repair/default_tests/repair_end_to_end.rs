//! End-to-end tests for default self-repair.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::Barrier;

use crate::agent::self_repair::default::DefaultSelfRepair;
use crate::agent::self_repair::{BrokenTool, NativeSelfRepair, RepairResult};
use crate::context::ContextManager;
use crate::error::DatabaseError;
use crate::testing::null_db::CapturingStore;

use super::helpers::{StubBuilderOutcome, StubSoftwareBuilder};

#[cfg(any(test, feature = "self_repair_extras"))]
#[tokio::test]
async fn repair_broken_tool_end_to_end_returns_success() {
    let cm = Arc::new(ContextManager::new(10));
    let store = Arc::new(CapturingStore::new());
    let store_for_repair: Arc<dyn crate::db::Database> = store.clone();

    let builder = Arc::new(
        StubSoftwareBuilder::new(StubBuilderOutcome::BuildSucceeded {
            is_success: true,
            error: None,
            iterations: 1,
            is_registered: false,
        })
        .with_build_barrier(Arc::new(Barrier::new(1))),
    );
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

/// Constructs a [`DefaultSelfRepair`] instance wired with a [`CapturingStore`]
/// and a [`Barrier`]-gated claim-overlap barrier, ready for concurrency tests.
fn build_concurrent_repair_fixture() -> (Arc<DefaultSelfRepair>, Arc<CapturingStore>) {
    let cm = Arc::new(ContextManager::new(10));
    let store = Arc::new(CapturingStore::new());
    let store_for_repair: Arc<dyn crate::db::Database> = store.clone();

    let claim_overlap = Arc::new(Barrier::new(2));
    let builder = Arc::new(StubSoftwareBuilder::new(
        StubBuilderOutcome::BuildSucceeded {
            is_success: true,
            error: None,
            iterations: 1,
            is_registered: false,
        },
    ));
    let builder_for_repair: Arc<dyn crate::tools::SoftwareBuilder> = builder;
    let tools = Arc::new(crate::tools::ToolRegistry::new());

    let repair = Arc::new(
        DefaultSelfRepair::new(cm, Duration::from_secs(60), 3)
            .with_store(store_for_repair)
            .with_builder(builder_for_repair, tools)
            .with_claim_overlap_barrier(claim_overlap),
    );

    (repair, store)
}

/// Spawns two concurrent [`NativeSelfRepair::repair_broken_tool`] calls for
/// the same `broken` tool and returns both results once both tasks complete.
async fn run_concurrent_repairs(
    repair: Arc<DefaultSelfRepair>,
    broken: Arc<BrokenTool>,
) -> [RepairResult; 2] {
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
    [
        first.expect("first repair_broken_tool call should not error"),
        second.expect("second repair_broken_tool call should not error"),
    ]
}

#[cfg(any(test, feature = "self_repair_extras"))]
#[tokio::test]
async fn repair_broken_tool_allows_one_concurrent_repair_for_same_tool() {
    let (repair, store) = build_concurrent_repair_fixture();

    let broken = Arc::new(BrokenTool {
        name: "my-tool".to_string(),
        failure_count: 2,
        last_error: Some("test error".to_string()),
        first_failure: Utc::now(),
        last_failure: Utc::now(),
        last_build_result: None,
        repair_attempts: 0,
    });

    let results = run_concurrent_repairs(repair, broken).await;

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

#[cfg(any(test, feature = "self_repair_extras"))]
#[tokio::test]
async fn repair_broken_tool_returns_retry_when_build_fails() {
    let cm = Arc::new(ContextManager::new(10));
    let store = Arc::new(CapturingStore::new());
    let store_for_repair: Arc<dyn crate::db::Database> = store.clone();

    let builder = Arc::new(StubSoftwareBuilder::new(
        StubBuilderOutcome::BuildSucceeded {
            is_success: false,
            error: Some("compilation failed"),
            iterations: 2,
            is_registered: false,
        },
    ));
    let builder_for_repair: Arc<dyn crate::tools::SoftwareBuilder> = builder;
    let tools = Arc::new(crate::tools::ToolRegistry::new());

    let repair = DefaultSelfRepair::new(cm, Duration::from_secs(60), 3)
        .with_store(store_for_repair)
        .with_builder(builder_for_repair, tools);

    let broken = BrokenTool {
        name: "my-tool".to_string(),
        failure_count: 2,
        last_error: Some("old error".to_string()),
        first_failure: Utc::now(),
        last_failure: Utc::now(),
        last_build_result: None,
        repair_attempts: 0,
    };

    let result = NativeSelfRepair::repair_broken_tool(&repair, &broken)
        .await
        .expect("repair_broken_tool should not error on build failure");

    assert!(
        matches!(result, RepairResult::Retry { .. }),
        "expected RepairResult::Retry when build fails, got: {result:?}",
    );
    // Tool must NOT be marked repaired when the build fails.
    assert!(
        store.calls().repaired_tools.lock().await.is_empty(),
        "failed build must not mark the tool as repaired",
    );
}

#[cfg(any(test, feature = "self_repair_extras"))]
#[tokio::test]
async fn repair_broken_tool_propagates_increment_repair_attempts_failure() {
    let cm = Arc::new(ContextManager::new(10));
    // Configure the store to fail `increment_repair_attempts` on the first call.
    let store = Arc::new(CapturingStore::failing_increment_repair_attempts_once(
        DatabaseError::NotFound {
            entity: "tool_failure".to_string(),
            id: "simulated increment failure".to_string(),
        },
    ));
    let store_for_repair: Arc<dyn crate::db::Database> = store.clone();

    let builder = Arc::new(StubSoftwareBuilder::new(
        StubBuilderOutcome::BuildSucceeded {
            is_success: true,
            error: None,
            iterations: 1,
            is_registered: false,
        },
    ));
    let builder_for_repair: Arc<dyn crate::tools::SoftwareBuilder> = builder;
    let tools = Arc::new(crate::tools::ToolRegistry::new());

    let repair = DefaultSelfRepair::new(cm, Duration::from_secs(60), 3)
        .with_store(store_for_repair)
        .with_builder(builder_for_repair, tools);

    let broken = BrokenTool {
        name: "my-tool".to_string(),
        failure_count: 2,
        last_error: None,
        first_failure: Utc::now(),
        last_failure: Utc::now(),
        last_build_result: None,
        repair_attempts: 0,
    };

    let result = NativeSelfRepair::repair_broken_tool(&repair, &broken).await;

    assert!(
        result.is_err(),
        "repair_broken_tool must propagate increment_repair_attempts failure as Err, got: {result:?}",
    );
    assert!(
        matches!(
            result.unwrap_err(),
            crate::error::RepairError::Failed { .. }
        ),
        "increment_repair_attempts error must surface as RepairError::Failed",
    );
}

#[cfg(any(test, feature = "self_repair_extras"))]
#[tokio::test]
async fn repair_broken_tool_propagates_mark_repaired_failure() {
    let cm = Arc::new(ContextManager::new(10));
    let store = Arc::new(CapturingStore::failing_mark_tool_repaired_once(
        DatabaseError::NotFound {
            entity: "tool_failure".to_string(),
            id: "simulated mark failure".to_string(),
        },
    ));
    let store_for_repair: Arc<dyn crate::db::Database> = store.clone();

    let builder = Arc::new(StubSoftwareBuilder::new(
        StubBuilderOutcome::BuildSucceeded {
            is_success: true,
            error: None,
            iterations: 1,
            is_registered: false,
        },
    ));
    let builder_for_repair: Arc<dyn crate::tools::SoftwareBuilder> = builder;
    let tools = Arc::new(crate::tools::ToolRegistry::new());

    let repair = DefaultSelfRepair::new(cm, Duration::from_secs(60), 3)
        .with_store(store_for_repair)
        .with_builder(builder_for_repair, tools);

    let broken = BrokenTool {
        name: "my-tool".to_string(),
        failure_count: 2,
        last_error: None,
        first_failure: Utc::now(),
        last_failure: Utc::now(),
        last_build_result: None,
        repair_attempts: 0,
    };

    let result = NativeSelfRepair::repair_broken_tool(&repair, &broken).await;

    assert!(
        result.is_err(),
        "repair_broken_tool must propagate mark_tool_repaired failure as Err, got: {result:?}",
    );
    assert!(
        matches!(
            result.unwrap_err(),
            crate::error::RepairError::Failed { .. }
        ),
        "mark_tool_repaired error must surface as RepairError::Failed",
    );
}
