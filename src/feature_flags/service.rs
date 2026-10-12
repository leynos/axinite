//! Hydration and override commands depend only on the feature-flag store port.

use std::future::Future;
use tokio::sync::RwLock;

use super::{DeploymentId, FlagName, registry::FeatureFlagRegistry};

/// Minimal persistence dependency for deployment feature flags.
pub trait FlagStore: Send + Sync {
    /// Adapter-specific failure propagated without changing its cause.
    type Error: std::error::Error + Send + Sync;

    /// Load all operator overrides for a validated deployment.
    fn load(
        &self,
        deployment: &DeploymentId,
    ) -> impl Future<Output = Result<Vec<(String, bool)>, Self::Error>> + Send;

    /// Persist one validated override.
    fn save(
        &self,
        deployment: &DeploymentId,
        flag: &FlagName,
        enabled: bool,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send;
}

/// Load once on success; failed loads leave the deployment eligible for retry.
pub async fn hydrate<S: FlagStore>(
    registry: &RwLock<FeatureFlagRegistry>,
    store: &S,
    deployment: &DeploymentId,
) -> Result<(), S::Error> {
    if registry.read().await.is_hydrated(deployment.as_str()) {
        return Ok(());
    }
    match store.load(deployment).await {
        Ok(overrides) => {
            registry
                .write()
                .await
                .hydrate(deployment.as_str().to_owned(), overrides);
            Ok(())
        }
        Err(error) => {
            registry.write().await.hydration_failed(deployment.as_str());
            Err(error)
        }
    }
}

/// A validated operator command, independent of HTTP and persistence adapters.
pub struct FlagOverride<'a> {
    /// Deployment whose operator override changes.
    pub deployment: &'a DeploymentId,
    /// Syntactically validated flag name.
    pub flag: &'a FlagName,
    /// Desired operator override; higher environment precedence still applies.
    pub enabled: bool,
}

/// Hydrate before writing so a write never hides existing persisted overrides.
/// Cache updates follow successful persistence only.
pub async fn set_override<S: FlagStore>(
    registry: &RwLock<FeatureFlagRegistry>,
    store: &S,
    command: FlagOverride<'_>,
) -> Result<(), S::Error> {
    hydrate(registry, store, command.deployment).await?;
    store
        .save(command.deployment, command.flag, command.enabled)
        .await?;
    registry.write().await.set(
        command.deployment.as_str().to_owned(),
        command.flag.as_str().to_owned(),
        command.enabled,
    );
    Ok(())
}

#[cfg(test)]
mod tests;
