//! Container lifecycle management for sandboxed jobs.
//!
//! Extends the existing `SandboxManager` infrastructure to support persistent
//! containers with their own agent loops (as opposed to ephemeral per-command containers).

use std::path::PathBuf;

use uuid::Uuid;

#[cfg(any(feature = "docker", test))]
use std::sync::Arc;
#[cfg(any(feature = "docker", test))]
use tokio::sync::RwLock;

use crate::error::OrchestratorError;
use crate::orchestrator::auth::{CredentialGrant, TokenStore};
use crate::orchestrator::job_registry::JobRegistry;
pub use crate::orchestrator::job_types::*;

#[cfg(feature = "docker")]
mod docker;
#[cfg(not(feature = "docker"))]
mod no_docker;
#[cfg(test)]
mod tests;

/// All inputs needed to create and start a single container job.
///
/// Passed as a single parameter to `create_job_inner` to avoid an
/// excess-arguments violation (CodeScene threshold = 4).
pub(super) struct CreateJobSpec {
    pub job_id: Uuid,
    /// Auth token minted by the TokenStore for this job.
    pub token: String,
    pub project_dir: Option<PathBuf>,
    pub mode: JobMode,
}

#[cfg(feature = "docker")]
pub use docker::ContainerJobManager;
#[cfg(not(feature = "docker"))]
pub use no_docker::ContainerJobManager;

impl ContainerJobManager {
    /// Create and start a new container for a job.
    ///
    /// The caller provides the `job_id` so it can be persisted to the database
    /// before the container is created. Credential grants are stored in the
    /// TokenStore and served on-demand via the `/credentials` endpoint.
    /// Returns the auth token for the worker.
    pub async fn create_job(
        &self,
        job_id: Uuid,
        task: &str,
        project_dir: Option<PathBuf>,
        mode: JobMode,
        credential_grants: Vec<CredentialGrant>,
    ) -> Result<String, OrchestratorError> {
        // Generate auth token (stored in TokenStore, never logged)
        let token = self.token_store.create_token(job_id).await;

        // Store credential grants (revoked automatically when the token is revoked)
        self.token_store
            .store_grants(job_id, credential_grants)
            .await;

        // Record the handle
        let handle = ContainerHandle {
            job_id,
            container_id: String::new(), // set after container creation
            state: ContainerState::Creating,
            mode,
            created_at: chrono::Utc::now(),
            project_dir: project_dir.clone(),
            task_description: task.to_string(),
            last_worker_status: None,
            worker_iteration: 0,
            completion_result: None,
        };
        self.registry.insert(handle).await;

        // Run the actual container creation. On any failure, revoke the token
        // and remove the handle so we don't leak resources.
        match self
            .create_job_inner(CreateJobSpec {
                job_id,
                token: token.clone(),
                project_dir,
                mode,
            })
            .await
        {
            Ok(()) => Ok(token),
            Err(e) => {
                self.token_store.revoke(job_id).await;
                self.registry.remove(job_id).await;
                Err(e)
            }
        }
    }
}

impl ContainerJobManager {
    /// Remove a completed job handle from memory (called after result is read).
    pub async fn cleanup_job(&self, job_id: Uuid) {
        self.registry.remove(job_id).await;
    }

    /// Update the worker-reported status for a job.
    pub async fn update_worker_status(
        &self,
        job_id: Uuid,
        message: Option<String>,
        iteration: u32,
    ) {
        self.registry
            .update_worker_status(job_id, message, iteration)
            .await;
    }

    /// Get the handle for a job.
    pub async fn get_handle(&self, job_id: Uuid) -> Option<ContainerHandle> {
        self.registry.get(job_id).await
    }

    /// List all active container jobs.
    pub async fn list_jobs(&self) -> Vec<ContainerHandle> {
        self.registry.list().await
    }

    /// Compatibility shim for in-module consumers that still need the raw map.
    #[cfg(any(feature = "docker", test))]
    pub(crate) fn containers(
        &self,
    ) -> Arc<RwLock<std::collections::HashMap<Uuid, ContainerHandle>>> {
        self.registry.arc()
    }

    /// Get a reference to the token store.
    pub fn token_store(&self) -> &TokenStore {
        &self.token_store
    }
}
