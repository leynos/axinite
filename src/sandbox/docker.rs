//! Docker connection caching for [`SandboxManager`].
//!
//! This module keeps Docker discovery, responsiveness checks, and cached client
//! access out of the higher-level sandbox coordinator so the manager can focus
//! on proxy and execution flow.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::sandbox::container::{
    DockerConnection, connect_docker, docker_is_responsive, ensure_docker_responsive,
};
use crate::sandbox::error::{Result, SandboxError};

/// Shared Docker connection cache for the sandbox manager.
pub(crate) struct DockerState {
    inner: Arc<RwLock<Option<DockerConnection>>>,
}

impl DockerState {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
        }
    }

    pub(crate) async fn get(&self) -> Result<DockerConnection> {
        self.inner
            .read()
            .await
            .clone()
            .ok_or_else(|| SandboxError::DockerNotAvailable {
                reason: "Docker connection not initialized".to_string(),
            })
    }

    pub(crate) async fn is_available(&self) -> bool {
        {
            let guard = self.inner.read().await;
            if let Some(ref docker) = *guard {
                return docker_is_responsive(docker).await;
            }
        }

        match connect_docker().await {
            Ok(docker) => {
                let is_responsive = docker_is_responsive(&docker).await;
                if is_responsive {
                    *self.inner.write().await = Some(docker);
                }
                is_responsive
            }
            Err(_) => false,
        }
    }

    pub(crate) async fn connect_verified(&self) -> Result<DockerConnection> {
        let docker = connect_docker().await?;
        ensure_docker_responsive(&docker).await?;
        Ok(docker)
    }

    pub(crate) async fn store(&self, docker: DockerConnection) {
        *self.inner.write().await = Some(docker);
    }
}
