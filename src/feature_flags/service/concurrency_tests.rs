//! Controlled store completion proves deployment ordering without scheduler sleeps.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::Poll;

use tokio::sync::{Mutex, Notify};

use super::*;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("injected store failure")]
struct StoreFailure;

#[derive(Default)]
struct ControlledStore {
    durable: Mutex<HashMap<(String, String), bool>>,
    writes: AtomicUsize,
    pause_production_disable: bool,
    release_first_write: Notify,
    fail_next_load: AtomicBool,
    fail_next_save: AtomicBool,
}

impl FlagStore for ControlledStore {
    type Error = StoreFailure;

    async fn load(&self, deployment: &DeploymentId) -> Result<Vec<(String, bool)>, Self::Error> {
        if self.fail_next_load.swap(false, Ordering::Relaxed) {
            return Err(StoreFailure);
        }
        Ok(self
            .durable
            .lock()
            .await
            .iter()
            .filter(|((id, _), _)| id == deployment.as_str())
            .map(|((_, flag), enabled)| (flag.clone(), *enabled))
            .collect())
    }

    async fn save(
        &self,
        deployment: &DeploymentId,
        flag: &FlagName,
        enabled: bool,
    ) -> Result<(), Self::Error> {
        if self.fail_next_save.swap(false, Ordering::Relaxed) {
            return Err(StoreFailure);
        }
        self.durable.lock().await.insert(
            (deployment.as_str().to_owned(), flag.as_str().to_owned()),
            enabled,
        );
        self.writes.fetch_add(1, Ordering::Relaxed);
        // The durable write has completed, but its command cannot publish yet.
        if self.pause_production_disable && is_production_disable(deployment, enabled) {
            self.release_first_write.notified().await;
        }
        Ok(())
    }
}

fn is_production_disable(deployment: &DeploymentId, enabled: bool) -> bool {
    deployment.as_str() == "production" && !enabled
}

fn override_command<'a>(
    deployment: &'a DeploymentId,
    flag: &'a FlagName,
    enabled: bool,
) -> FlagOverride<'a> {
    FlagOverride {
        deployment,
        flag,
        enabled,
    }
}

#[tokio::test]
async fn same_deployment_writes_preserve_persistence_and_publication_order() {
    let deployment = DeploymentId::parse("production").expect("valid deployment");
    let flag = FlagName::parse("panel_logs").expect("valid flag");
    let registry = RwLock::new(FeatureFlagRegistry::new());
    let store = ControlledStore {
        pause_production_disable: true,
        ..Default::default()
    };
    let mut first = Box::pin(set_override(
        &registry,
        &store,
        override_command(&deployment, &flag, false),
    ));
    assert!(futures::poll!(first.as_mut()).is_pending());
    assert_eq!(
        store.writes.load(Ordering::Relaxed),
        1,
        "first write is already durable"
    );

    let mut second = Box::pin(set_override(
        &registry,
        &store,
        override_command(&deployment, &flag, true),
    ));
    assert!(
        futures::poll!(second.as_mut()).is_pending(),
        "second write must await first publication"
    );
    assert_eq!(store.writes.load(Ordering::Relaxed), 1);
    // Cache access remains available while storage has paused the first command.
    assert!(
        registry.try_read().is_ok(),
        "storage must not retain the registry lock"
    );
    assert_eq!(registry.read().await.get("production", "panel_logs"), None);
    store.release_first_write.notify_one();
    first.await.expect("first command completes");
    second.await.expect("second command completes");
    assert_eq!(store.writes.load(Ordering::Relaxed), 2);
    assert_eq!(
        store
            .durable
            .lock()
            .await
            .get(&("production".to_owned(), "panel_logs".to_owned())),
        Some(&true)
    );
    assert_eq!(
        registry.read().await.get("production", "panel_logs"),
        Some(true)
    );
}

#[tokio::test]
async fn another_deployment_progresses_while_a_write_is_waiting_on_storage() {
    let production = DeploymentId::parse("production").expect("valid production");
    let staging = DeploymentId::parse("staging").expect("valid staging");
    let flag = FlagName::parse("panel_logs").expect("valid flag");
    let registry = RwLock::new(FeatureFlagRegistry::new());
    let store = ControlledStore {
        pause_production_disable: true,
        ..Default::default()
    };
    let mut first = Box::pin(set_override(
        &registry,
        &store,
        override_command(&production, &flag, false),
    ));
    assert!(futures::poll!(first.as_mut()).is_pending());
    let mut independent = Box::pin(set_override(
        &registry,
        &store,
        override_command(&staging, &flag, true),
    ));
    assert_eq!(futures::poll!(independent.as_mut()), Poll::Ready(Ok(())));
    assert_eq!(
        registry.read().await.get("staging", "panel_logs"),
        Some(true)
    );
    store.release_first_write.notify_one();
    first.await.expect("production finishes after staging");
    assert_eq!(
        registry.read().await.get("production", "panel_logs"),
        Some(false)
    );
}

enum FailurePoint {
    Hydration,
    Persistence,
}

#[rstest::rstest]
#[case::hydration_failure(FailurePoint::Hydration)]
#[case::persistence_failure(FailurePoint::Persistence)]
#[tokio::test]
async fn failed_commands_release_deployment_admission(#[case] failure: FailurePoint) {
    let deployment = DeploymentId::parse("production").expect("valid deployment");
    let flag = FlagName::parse("panel_logs").expect("valid flag");
    let registry = RwLock::new(FeatureFlagRegistry::new());
    let store = ControlledStore::default();
    match failure {
        FailurePoint::Hydration => store.fail_next_load.store(true, Ordering::Relaxed),
        FailurePoint::Persistence => store.fail_next_save.store(true, Ordering::Relaxed),
    }
    let result = set_override(
        &registry,
        &store,
        override_command(&deployment, &flag, false),
    )
    .await;
    assert_eq!(result, Err(StoreFailure));
    assert_eq!(store.writes.load(Ordering::Relaxed), 0);
    let mut retry = Box::pin(set_override(
        &registry,
        &store,
        override_command(&deployment, &flag, true),
    ));
    assert_eq!(
        futures::poll!(retry.as_mut()),
        Poll::Ready(Ok(())),
        "failed command must release admission"
    );
    assert_eq!(store.writes.load(Ordering::Relaxed), 1);
    assert_eq!(
        registry.read().await.get("production", "panel_logs"),
        Some(true)
    );
}
