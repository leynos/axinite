//! Read boundaries return detached values without command-side cache transitions.

use super::*;
use crate::feature_flags::FlagName;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct SnapshotStore {
    snapshot: FlagSnapshot,
    loads: AtomicUsize,
    saves: AtomicUsize,
    fail_next_load: AtomicBool,
}

impl FlagStore for SnapshotStore {
    async fn load(&self, _: &DeploymentId) -> Result<FlagSnapshot, FlagStoreError> {
        self.loads.fetch_add(1, Ordering::Relaxed);
        if self.fail_next_load.swap(false, Ordering::Relaxed) {
            return Err(FlagStoreError::Load);
        }
        Ok(self.snapshot.clone())
    }
    async fn save(&self, _: &FlagOverrideCommand) -> Result<(), FlagStoreError> {
        self.saves.fetch_add(1, Ordering::Relaxed);
        Err(FlagStoreError::Save)
    }
}

#[tokio::test]
async fn repeated_uncached_queries_return_persisted_values_without_registry_publication() {
    let deployment = DeploymentId::parse("production").expect("valid deployment");
    let flag = FlagName::parse("route_memory").expect("valid flag");
    let registry = RwLock::new(FeatureFlagRegistry::new());
    let store = SnapshotStore {
        snapshot: FlagSnapshot::from_values([(flag.clone(), false)]),
        loads: AtomicUsize::new(0),
        saves: AtomicUsize::new(0),
        fail_next_load: AtomicBool::new(false),
    };
    let before = registry.read().await.resources();
    let mut first = query(None, Some(&store), &deployment)
        .await
        .expect("healthy query");
    assert_eq!(first.get("route_memory"), Some(false));
    first.set(flag, true);
    let second = query(None, Some(&store), &deployment)
        .await
        .expect("repeated query");
    assert_eq!(second.get("route_memory"), Some(false));
    assert_eq!(registry.read().await.resources(), before);
    assert_eq!(
        registry.read().await.get("production", "route_memory"),
        None
    );
    assert_eq!(store.loads.load(Ordering::Relaxed), 2);
    assert_eq!(store.saves.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn failed_query_retries_storage_without_recording_command_hydration_failure() {
    let deployment = DeploymentId::parse("production").expect("valid deployment");
    let registry = RwLock::new(FeatureFlagRegistry::new());
    let store = SnapshotStore {
        snapshot: FlagSnapshot::default(),
        loads: AtomicUsize::new(0),
        saves: AtomicUsize::new(0),
        fail_next_load: AtomicBool::new(true),
    };
    let before = registry.read().await.resources();
    assert_eq!(
        query(None, Some(&store), &deployment).await,
        Err(FlagStoreError::Load)
    );
    assert_eq!(registry.read().await.resources(), before);
    assert!(!registry.read().await.is_retry("production"));
    assert_eq!(
        query(None, Some(&store), &deployment).await,
        Ok(FlagSnapshot::default())
    );
    assert_eq!(registry.read().await.resources(), before);
    assert_eq!(store.loads.load(Ordering::Relaxed), 2);
    assert_eq!(store.saves.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn cached_queries_return_defensive_snapshots_without_mutating_failure_entries() {
    let deployment = DeploymentId::parse("production").expect("valid deployment");
    let failed = DeploymentId::parse("staging").expect("valid staging");
    let flag = FlagName::parse("route_memory").expect("valid flag");
    let registry = RwLock::new(FeatureFlagRegistry::new());
    registry.write().await.hydrate(
        deployment.clone(),
        FlagSnapshot::from_values([(flag.clone(), false)]),
    );
    registry.write().await.hydration_failed(&failed);
    let store = SnapshotStore {
        snapshot: FlagSnapshot::default(),
        loads: AtomicUsize::new(0),
        saves: AtomicUsize::new(0),
        fail_next_load: AtomicBool::new(true),
    };
    let before = registry.read().await.resources();
    let cached = registry.read().await.snapshot(&deployment);
    let mut returned = query(cached, Some(&store), &deployment)
        .await
        .expect("cached query");
    returned.set(flag, true);
    assert_eq!(registry.read().await.resources(), before);
    assert_eq!(
        registry.read().await.get("production", "route_memory"),
        Some(false)
    );
    assert!(registry.read().await.is_retry("staging"));
    assert_eq!(store.loads.load(Ordering::Relaxed), 0);
    assert_eq!(store.saves.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn detached_queries_emit_no_observer_events_for_cache_defaults_load_or_failure() {
    use super::super::telemetry::test_support::RecordingObserver;
    use tracing::instrument::WithSubscriber;

    let observer = RecordingObserver::default();
    let deployment = DeploymentId::parse("production").expect("valid deployment");
    let store = SnapshotStore {
        snapshot: FlagSnapshot::default(),
        loads: AtomicUsize::new(0),
        saves: AtomicUsize::new(0),
        fail_next_load: AtomicBool::new(false),
    };
    async {
        assert_eq!(
            query(Some(FlagSnapshot::default()), Some(&store), &deployment).await,
            Ok(FlagSnapshot::default())
        );
        assert_eq!(
            query::<SnapshotStore>(None, None, &deployment).await,
            Ok(FlagSnapshot::default())
        );
        assert_eq!(
            query(None, Some(&store), &deployment).await,
            Ok(FlagSnapshot::default())
        );
        store.fail_next_load.store(true, Ordering::Relaxed);
        assert_eq!(
            query(None, Some(&store), &deployment).await,
            Err(FlagStoreError::Load)
        );
    }
    .with_subscriber(observer.clone().subscriber())
    .await;
    assert!(
        observer.observations().is_empty(),
        "queries must not write telemetry"
    );
    assert_eq!(store.loads.load(Ordering::Relaxed), 2);
    assert_eq!(store.saves.load(Ordering::Relaxed), 0);
}
