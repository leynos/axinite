use super::*;

use crate::sandbox::container::DOCKER_FEATURE_DISABLED_REASON;

/// Manages the lifecycle of Docker containers for sandboxed job execution.
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
        job_id: Uuid,
        _token: &str,
        _project_dir: Option<PathBuf>,
        _mode: JobMode,
    ) -> Result<(), OrchestratorError> {
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

        if let Some(container_id) = self
            .registry
            .container_id(job_id)
            .await
            .filter(|container_id| !container_id.is_empty())
        {
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
