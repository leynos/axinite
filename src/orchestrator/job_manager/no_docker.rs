//! Docker-disabled container job orchestration.
//!
//! This module provides the [`ContainerJobManager`] fallback used when Docker
//! support is not compiled in. It preserves the sandbox job management surface
//! while returning errors based on `DOCKER_FEATURE_DISABLED_REASON`.

use super::*;

use crate::sandbox::container::DOCKER_FEATURE_DISABLED_REASON;

/// No-Docker backend for sandboxed job execution.
///
/// This implementation is compiled when Docker support is disabled and uses
/// `DOCKER_FEATURE_DISABLED_REASON` for operations that require containers.
pub struct ContainerJobManager {
    pub(super) token_store: TokenStore,
    pub(crate) registry: JobRegistry,
}

impl ContainerJobManager {
    pub fn new(_config: ContainerJobConfig, token_store: TokenStore) -> Self {
        Self {
            token_store,
            registry: JobRegistry::new(),
        }
    }

    pub(super) async fn create_job_inner(
        &self,
        params: CreateJobParams,
    ) -> Result<(), OrchestratorError> {
        let CreateJobParams {
            job_id,
            token,
            project_dir,
            mode,
        } = params;
        let _ = (token, project_dir, mode);
        Err(OrchestratorError::Docker {
            reason: format!("{DOCKER_FEATURE_DISABLED_REASON}, cannot create sandbox job {job_id}"),
        })
    }

    /// Stop a running container job.
    pub async fn stop_job(&self, job_id: Uuid) -> Result<(), OrchestratorError> {
        Err(OrchestratorError::Docker {
            reason: format!("{DOCKER_FEATURE_DISABLED_REASON}, cannot stop sandbox job {job_id}"),
        })
    }

    /// Mark a job as complete with a result. The container is stopped but the
    /// handle is kept so `CreateJobTool` can read the completion message.
    pub async fn complete_job(
        &self,
        job_id: Uuid,
        result: CompletionResult,
    ) -> Result<(), OrchestratorError> {
        self.registry.set_completion(job_id, result).await;

        if let Some(container_id) = self.registry.container_id(job_id).await {
            tracing::warn!(
                job_id = %job_id,
                container_id = %container_id,
                "Skipping completed container cleanup because Docker support was not compiled in"
            );
        }

        self.token_store.revoke(job_id).await;

        tracing::info!(job_id = %job_id, "Completed worker container");
        Ok(())
    }
}
