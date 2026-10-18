//! Independent durable/cache reference model and a controllable typed repository double.

use super::*;
use crate::feature_flags::FlagName;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use tokio::sync::{Mutex, Notify};

type Values = BTreeMap<String, bool>;

#[derive(Default)]
pub(super) struct ModelStore {
    pub durable: Mutex<BTreeMap<(String, String), bool>>,
    pub failure: AtomicU8,
    pub pause_next_save: AtomicBool,
    pub release_save: Notify,
}

impl FlagStore for ModelStore {
    async fn load(&self, deployment: &DeploymentId) -> Result<FlagSnapshot, FlagStoreError> {
        if self.failure.load(Ordering::Relaxed) == 1 {
            return Err(FlagStoreError::Load);
        }
        let values = self
            .durable
            .lock()
            .await
            .iter()
            .filter(|((id, _), _)| id == deployment.as_str())
            .map(|((_, flag), value)| {
                FlagName::parse(flag)
                    .map(|flag| (flag, *value))
                    .map_err(|_| FlagStoreError::InvalidSnapshot)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(FlagSnapshot::from_values(values))
    }

    async fn save(&self, command: &FlagOverrideCommand) -> Result<(), FlagStoreError> {
        if self.failure.load(Ordering::Relaxed) == 2 {
            return Err(FlagStoreError::Save);
        }
        self.durable.lock().await.insert(
            (
                command.deployment.as_str().to_owned(),
                command.flag.as_str().to_owned(),
            ),
            command.enabled,
        );
        if self.pause_next_save.swap(false, Ordering::Relaxed) {
            self.release_save.notified().await;
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct Reference {
    pub durable: BTreeMap<(String, String), bool>,
    pub cache: BTreeMap<String, Values>,
    failed: BTreeSet<String>,
    gates: BTreeSet<String>,
}

impl Reference {
    pub fn persisted(&self, deployment: &str) -> Values {
        self.durable
            .iter()
            .filter(|((id, _), _)| id == deployment)
            .map(|((_, flag), value)| (flag.clone(), *value))
            .collect()
    }

    pub fn queried(&self, deployment: &str, failure: u8) -> Result<Values, FlagStoreError> {
        if let Some(values) = self.cache.get(deployment) {
            return Ok(values.clone());
        }
        if failure == 1 {
            return Err(FlagStoreError::Load);
        }
        Ok(self.persisted(deployment))
    }

    pub fn begin_write(&mut self, deployment: &str, failure: u8) -> Result<(), FlagStoreError> {
        self.gates.insert(deployment.to_owned());
        if !self.cache.contains_key(deployment) {
            if failure == 1 {
                self.failed.insert(deployment.to_owned());
                return Err(FlagStoreError::Load);
            }
            self.cache
                .insert(deployment.to_owned(), self.persisted(deployment));
            self.failed.remove(deployment);
        }
        Ok(())
    }

    pub fn persist(&mut self, command: &FlagOverrideCommand) {
        self.durable.insert(
            (
                command.deployment.as_str().to_owned(),
                command.flag.as_str().to_owned(),
            ),
            command.enabled,
        );
    }

    pub fn publish(&mut self, command: &FlagOverrideCommand) {
        self.cache
            .entry(command.deployment.as_str().to_owned())
            .or_default()
            .insert(command.flag.as_str().to_owned(), command.enabled);
    }

    pub fn write(
        &mut self,
        command: &FlagOverrideCommand,
        failure: u8,
    ) -> Result<(), FlagStoreError> {
        self.begin_write(command.deployment.as_str(), failure)?;
        if failure == 2 {
            return Err(FlagStoreError::Save);
        }
        self.persist(command);
        self.publish(command);
        Ok(())
    }

    pub fn resources(&self) -> super::super::registry::CacheResources {
        super::super::registry::CacheResources {
            deployments: self.cache.len(),
            overrides: self.cache.values().map(Values::len).sum(),
            failed_hydrations: self.failed.len(),
            admission_gates: self.gates.len(),
        }
    }
}

pub(super) fn values(snapshot: &FlagSnapshot) -> Values {
    snapshot
        .values()
        .map(|(flag, enabled)| (flag.as_str().to_owned(), enabled))
        .collect()
}
