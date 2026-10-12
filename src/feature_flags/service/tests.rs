//! Command tests use an in-process port double with no database or HTTP state.

use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Debug, thiserror::Error)]
#[error("store unavailable")]
struct StoreUnavailable;

#[derive(Default)]
struct RecordingStore {
    loads: AtomicUsize,
    writes: AtomicUsize,
    fail_next_load: AtomicBool,
    fail_write: AtomicBool,
}

impl FlagStore for RecordingStore {
    type Error = StoreUnavailable;

    async fn load(&self, _: &DeploymentId) -> Result<Vec<(String, bool)>, Self::Error> {
        self.loads.fetch_add(1, Ordering::Relaxed);
        if self.fail_next_load.swap(false, Ordering::Relaxed) {
            return Err(StoreUnavailable);
        }
        Ok(vec![("route_memory".to_owned(), false)])
    }

    async fn save(&self, _: &DeploymentId, _: &FlagName, _: bool) -> Result<(), Self::Error> {
        self.writes.fetch_add(1, Ordering::Relaxed);
        if self.fail_write.load(Ordering::Relaxed) {
            return Err(StoreUnavailable);
        }
        Ok(())
    }
}

#[tokio::test]
async fn successful_hydration_queries_the_store_only_once() {
    let deployment = DeploymentId::parse("production").expect("valid deployment");
    let registry = RwLock::new(FeatureFlagRegistry::new());
    let store = RecordingStore::default();
    hydrate(&registry, &store, &deployment)
        .await
        .expect("first load");
    hydrate(&registry, &store, &deployment)
        .await
        .expect("cached load");
    assert_eq!(store.loads.load(Ordering::Relaxed), 1);
    assert_eq!(
        registry.read().await.get("production", "route_memory"),
        Some(false)
    );
}

#[tokio::test]
async fn failed_hydration_prevents_write_and_retries_without_marking_loaded() {
    let deployment = DeploymentId::parse("production").expect("valid deployment");
    let flag = FlagName::parse("panel_logs").expect("valid flag");
    let registry = RwLock::new(FeatureFlagRegistry::new());
    let store = RecordingStore::default();
    store.fail_next_load.store(true, Ordering::Relaxed);
    assert!(
        set_override(
            &registry,
            &store,
            FlagOverride {
                deployment: &deployment,
                flag: &flag,
                enabled: true
            }
        )
        .await
        .is_err()
    );
    assert_eq!(store.writes.load(Ordering::Relaxed), 0);
    assert!(!registry.read().await.is_hydrated("production"));
    assert!(registry.read().await.is_retry("production"));
    set_override(
        &registry,
        &store,
        FlagOverride {
            deployment: &deployment,
            flag: &flag,
            enabled: true,
        },
    )
    .await
    .expect("retry succeeds");
    let cache = registry.read().await;
    assert!(cache.is_hydrated("production"));
    assert!(!cache.is_retry("production"));
    assert_eq!(cache.get("production", "route_memory"), Some(false));
    assert_eq!(cache.get("production", "panel_logs"), Some(true));
    assert_eq!(store.loads.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn failed_persistence_keeps_the_last_cached_override() {
    let deployment = DeploymentId::parse("production").expect("valid deployment");
    let flag = FlagName::parse("panel_logs").expect("valid flag");
    let registry = RwLock::new(FeatureFlagRegistry::new());
    let store = RecordingStore::default();
    set_override(
        &registry,
        &store,
        FlagOverride {
            deployment: &deployment,
            flag: &flag,
            enabled: false,
        },
    )
    .await
    .expect("initial write");
    store.fail_write.store(true, Ordering::Relaxed);
    assert!(
        set_override(
            &registry,
            &store,
            FlagOverride {
                deployment: &deployment,
                flag: &flag,
                enabled: true
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        registry.read().await.get("production", "panel_logs"),
        Some(false)
    );
    assert_eq!(store.loads.load(Ordering::Relaxed), 1);
}
