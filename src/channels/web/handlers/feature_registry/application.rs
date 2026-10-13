//! Gateway application queries and commands use typed flag snapshots and a repository port.
//!
//! Queries return detached cached or locally loaded snapshots and never create
//! admission gates or publish hydration/retry state. Explicit commands own those
//! transitions and retain per-deployment admission through durable write publication.

use super::{FeatureFlagRegistry, telemetry};
use crate::feature_flags::{DeploymentId, FlagOverrideCommand, FlagSnapshot};
use std::future::Future;
use tokio::sync::{OwnedMutexGuard, RwLock};

/// Persistence failures translated by the settings adapter, without database types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum FlagStoreError {
    /// The repository could not read the deployment snapshot.
    #[error("feature flag snapshot unavailable")]
    Load,
    /// The repository could not durably save the operator command.
    #[error("feature flag override could not be persisted")]
    Save,
    /// Persisted input failed validation at the adapter boundary.
    #[error("feature flag snapshot contains an invalid flag name")]
    InvalidSnapshot,
}

/// Minimal repository port expressed in domain values, not database rows.
pub(crate) trait FlagStore: Send + Sync {
    /// Load a detached validated snapshot for the deployment.
    fn load(
        &self,
        deployment: &DeploymentId,
    ) -> impl Future<Output = Result<FlagSnapshot, FlagStoreError>> + Send;
    /// Persist one validated operator command.
    fn save(
        &self,
        command: &FlagOverrideCommand,
    ) -> impl Future<Output = Result<(), FlagStoreError>> + Send;
}

/// Resolve a detached cached value or load locally without registry access.
pub(crate) async fn query<S: FlagStore>(
    cached: Option<FlagSnapshot>,
    store: Option<&S>,
    deployment: &DeploymentId,
) -> Result<FlagSnapshot, FlagStoreError> {
    if let Some(snapshot) = cached {
        return Ok(snapshot);
    }
    let Some(store) = store else {
        return Ok(FlagSnapshot::default());
    };
    store.load(deployment).await
}

/// Acquire only operation admission across I/O; release the registry lock first.
async fn admit_deployment(
    registry: &RwLock<FeatureFlagRegistry>,
    deployment: &DeploymentId,
) -> OwnedMutexGuard<()> {
    let started = std::time::Instant::now();
    let gate = registry.write().await.operation_gate(deployment);
    let guard = gate.lock_owned().await;
    telemetry::admission_wait(started);
    guard
}

/// Explicit command hydration; queries never invoke this mutating path.
async fn hydrate_admitted<S: FlagStore>(
    registry: &RwLock<FeatureFlagRegistry>,
    store: &S,
    deployment: &DeploymentId,
) -> Result<(), FlagStoreError> {
    let (is_hydrated, is_retry) = {
        let cache = registry.read().await;
        (
            cache.is_hydrated(deployment.as_str()),
            cache.is_retry(deployment.as_str()),
        )
    };
    if is_hydrated {
        telemetry::count("cache", "hit");
        return Ok(());
    }
    telemetry::count("cache", "miss");
    if is_retry {
        telemetry::count("hydration", "retry");
    }
    let started = std::time::Instant::now();
    let loaded = store.load(deployment).await;
    telemetry::store_result("load", started, loaded.is_ok());
    match loaded {
        Ok(snapshot) => {
            registry.write().await.hydrate(deployment.clone(), snapshot);
            telemetry::count("hydration", "success");
            Ok(())
        }
        Err(error) => {
            registry.write().await.hydration_failed(deployment);
            telemetry::count("hydration", "error");
            Err(error)
        }
    }
}

/// Hydrate, persist, and publish under one deployment's owned admission guard.
/// No registry lock spans storage I/O, and errors release admission for retries.
/// Cancellation after durable persistence remains a separate consistency boundary.
#[tracing::instrument(name = "feature_flags.override_command", skip_all,
    fields(operation_id = %uuid::Uuid::new_v4()))]
pub(crate) async fn set_override<S: FlagStore>(
    registry: &RwLock<FeatureFlagRegistry>,
    store: &S,
    command: FlagOverrideCommand,
) -> Result<(), FlagStoreError> {
    let started = std::time::Instant::now();
    let _operation = admit_deployment(registry, &command.deployment).await;
    let result = apply_admitted(registry, store, &command).await;
    let resources = registry.read().await.resources();
    telemetry::cache_resources(resources);
    telemetry::command_result(started, result.as_ref().err().copied());
    result
}

/// Apply under caller-owned deployment admission: hydrate before saving, then
/// publish only a successful durable save. Propagate failures without publishing
/// the command value; cancellation after a durable save has no stronger guarantee.
async fn apply_admitted<S: FlagStore>(
    registry: &RwLock<FeatureFlagRegistry>,
    store: &S,
    command: &FlagOverrideCommand,
) -> Result<(), FlagStoreError> {
    hydrate_admitted(registry, store, &command.deployment).await?;
    store.save(command).await?;
    registry.write().await.set(command);
    Ok(())
}

#[cfg(test)]
mod concurrency_tests;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod model_support;
