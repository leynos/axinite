//! In-memory registry of active container job handles.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::orchestrator::job_types::{
    CompletionResult, ContainerHandle, ContainerId, ContainerState,
};

pub(crate) struct JobRegistry {
    inner: Arc<RwLock<HashMap<Uuid, ContainerHandle>>>,
}

impl JobRegistry {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub(crate) async fn insert(&self, handle: ContainerHandle) {
        self.inner.write().await.insert(handle.job_id, handle);
    }

    pub(crate) async fn remove(&self, job_id: Uuid) -> Option<ContainerHandle> {
        self.inner.write().await.remove(&job_id)
    }

    pub(crate) async fn get(&self, job_id: Uuid) -> Option<ContainerHandle> {
        self.inner.read().await.get(&job_id).cloned()
    }

    pub(crate) async fn list(&self) -> Vec<ContainerHandle> {
        self.inner.read().await.values().cloned().collect()
    }

    pub(crate) async fn update_worker_status(
        &self,
        job_id: Uuid,
        message: Option<String>,
        iteration: u32,
    ) {
        if let Some(handle) = self.inner.write().await.get_mut(&job_id) {
            handle.last_worker_status = message;
            handle.worker_iteration = iteration;
        }
    }

    pub(crate) async fn set_completion(&self, job_id: Uuid, result: CompletionResult) {
        if let Some(handle) = self.inner.write().await.get_mut(&job_id) {
            handle.completion_result = Some(result);
            handle.state = ContainerState::Stopped;
        }
    }

    #[cfg(feature = "docker")]
    pub(crate) async fn set_container_id(&self, job_id: Uuid, container_id: ContainerId) {
        if let Some(handle) = self.inner.write().await.get_mut(&job_id) {
            handle.container_id = Some(container_id);
            handle.state = ContainerState::Running;
        }
    }

    #[cfg(feature = "docker")]
    pub(crate) async fn set_state(&self, job_id: Uuid, state: ContainerState) {
        if let Some(handle) = self.inner.write().await.get_mut(&job_id) {
            handle.state = state;
        }
    }

    pub(crate) async fn container_id(&self, job_id: Uuid) -> Option<ContainerId> {
        self.inner
            .read()
            .await
            .get(&job_id)
            .and_then(|handle| handle.container_id.clone())
    }

    #[cfg(test)]
    pub(crate) fn arc(&self) -> Arc<RwLock<HashMap<Uuid, ContainerHandle>>> {
        Arc::clone(&self.inner)
    }
}
