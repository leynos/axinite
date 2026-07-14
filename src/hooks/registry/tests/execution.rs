//! Registration, ordering, and failure-mode behaviour tests for the registry.

use std::sync::Arc;

use crate::hooks::hook::{HookError, HookFailureMode, HookOutcome, HookPoint};
use crate::hooks::registry::HookRegistry;

use super::harness::{ErrorHook, ModifyHook, PassthroughHook, RejectHook, SlowHook, test_event};

#[tokio::test]
async fn test_empty_registry_returns_ok() {
    let registry = HookRegistry::new();
    let result = registry.run(&test_event()).await;
    assert!(result.is_ok());
    assert!(matches!(
        result.unwrap(),
        HookOutcome::Continue { modified: None }
    ));
}

#[tokio::test]
async fn test_register_and_list() {
    let registry = HookRegistry::new();
    registry
        .register(Arc::new(PassthroughHook {
            name: "hook-a".into(),
            points: vec![HookPoint::BeforeInbound],
        }))
        .await;
    registry
        .register(Arc::new(PassthroughHook {
            name: "hook-b".into(),
            points: vec![HookPoint::BeforeInbound],
        }))
        .await;

    let names = registry.list().await;
    assert_eq!(names, vec!["hook-a", "hook-b"]);
}

#[tokio::test]
async fn test_register_duplicate_name_replaces_existing() {
    let registry = HookRegistry::new();

    registry
        .register_with_priority(
            Arc::new(ModifyHook {
                name: "dup".into(),
                suffix: "-A".into(),
                points: vec![HookPoint::BeforeInbound],
            }),
            100,
        )
        .await;

    registry
        .register_with_priority(
            Arc::new(ModifyHook {
                name: "dup".into(),
                suffix: "-B".into(),
                points: vec![HookPoint::BeforeInbound],
            }),
            10,
        )
        .await;

    let names = registry.list().await;
    assert_eq!(names, vec!["dup"]);

    let result = registry.run(&test_event()).await.unwrap();
    match result {
        HookOutcome::Continue {
            modified: Some(value),
        } => assert_eq!(value, "hello-B"),
        other => panic!("expected modified output, got {other:?}"),
    }
}

#[tokio::test]
async fn test_priority_ordering() {
    let registry = HookRegistry::new();

    // Register in reverse priority order
    registry
        .register_with_priority(
            Arc::new(ModifyHook {
                name: "low-prio".into(),
                suffix: "-LOW".into(),
                points: vec![HookPoint::BeforeInbound],
            }),
            200,
        )
        .await;
    registry
        .register_with_priority(
            Arc::new(ModifyHook {
                name: "high-prio".into(),
                suffix: "-HIGH".into(),
                points: vec![HookPoint::BeforeInbound],
            }),
            10,
        )
        .await;

    // Should run in priority order: high-prio first, then low-prio
    let names = registry.list().await;
    assert_eq!(names[0], "high-prio");
    assert_eq!(names[1], "low-prio");

    let result = registry.run(&test_event()).await.unwrap();
    match result {
        HookOutcome::Continue { modified: Some(m) } => {
            // "hello" -> "hello-HIGH" -> "hello-HIGH-LOW"
            assert_eq!(m, "hello-HIGH-LOW");
        }
        other => panic!("Expected modification chain, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_reject_stops_chain() {
    let registry = HookRegistry::new();

    registry
        .register_with_priority(
            Arc::new(RejectHook {
                name: "blocker".into(),
                reason: "blocked".into(),
                points: vec![HookPoint::BeforeInbound],
            }),
            10,
        )
        .await;
    registry
        .register_with_priority(
            Arc::new(ModifyHook {
                name: "modifier".into(),
                suffix: "-MODIFIED".into(),
                points: vec![HookPoint::BeforeInbound],
            }),
            20,
        )
        .await;

    let result = registry.run(&test_event()).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        HookError::Rejected { reason } => assert_eq!(reason, "blocked"),
        other => panic!("Expected Rejected, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_modification_chaining() {
    let registry = HookRegistry::new();

    registry
        .register_with_priority(
            Arc::new(ModifyHook {
                name: "first".into(),
                suffix: "-A".into(),
                points: vec![HookPoint::BeforeInbound],
            }),
            10,
        )
        .await;
    registry
        .register_with_priority(
            Arc::new(ModifyHook {
                name: "second".into(),
                suffix: "-B".into(),
                points: vec![HookPoint::BeforeInbound],
            }),
            20,
        )
        .await;

    let result = registry.run(&test_event()).await.unwrap();
    match result {
        HookOutcome::Continue { modified: Some(m) } => {
            assert_eq!(m, "hello-A-B");
        }
        other => panic!("Expected chained modification, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_fail_open_on_error() {
    let registry = HookRegistry::new();
    registry
        .register(Arc::new(ErrorHook {
            name: "err-open".into(),
            points: vec![HookPoint::BeforeInbound],
            failure_mode: HookFailureMode::FailOpen,
        }))
        .await;

    let result = registry.run(&test_event()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_fail_closed_on_error() {
    let registry = HookRegistry::new();
    registry
        .register(Arc::new(ErrorHook {
            name: "err-closed".into(),
            points: vec![HookPoint::BeforeInbound],
            failure_mode: HookFailureMode::FailClosed,
        }))
        .await;

    let result = registry.run(&test_event()).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        HookError::ExecutionFailed { .. }
    ));
}

#[tokio::test]
async fn test_fail_open_on_timeout() {
    let registry = HookRegistry::new();
    registry
        .register(Arc::new(SlowHook {
            name: "slow-open".into(),
            points: vec![HookPoint::BeforeInbound],
            failure_mode: HookFailureMode::FailOpen,
        }))
        .await;

    let result = registry.run(&test_event()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_fail_closed_on_timeout() {
    let registry = HookRegistry::new();
    registry
        .register(Arc::new(SlowHook {
            name: "slow-closed".into(),
            points: vec![HookPoint::BeforeInbound],
            failure_mode: HookFailureMode::FailClosed,
        }))
        .await;

    let result = registry.run(&test_event()).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), HookError::Timeout { .. }));
}

#[tokio::test]
async fn test_unregister() {
    let registry = HookRegistry::new();
    registry
        .register(Arc::new(PassthroughHook {
            name: "removable".into(),
            points: vec![HookPoint::BeforeInbound],
        }))
        .await;

    assert_eq!(registry.list().await.len(), 1);
    assert!(registry.unregister("removable").await);
    assert_eq!(registry.list().await.len(), 0);

    // Unregistering non-existent returns false
    assert!(!registry.unregister("nonexistent").await);
}

#[tokio::test]
async fn test_hooks_only_match_their_points() {
    let registry = HookRegistry::new();
    registry
        .register(Arc::new(RejectHook {
            name: "outbound-only".into(),
            reason: "blocked".into(),
            points: vec![HookPoint::BeforeOutbound],
        }))
        .await;

    // Inbound event should not be affected by outbound-only hook
    let result = registry.run(&test_event()).await;
    assert!(result.is_ok());
}
