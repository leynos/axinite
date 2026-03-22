//! Docker-backed container job orchestration.
//!
//! This module implements [`ContainerJobManager`] for sandboxed job execution
//! when Docker support is enabled. It caches a [`DockerConnection`] from
//! `connect_docker()`, applies [`ContainerJobConfig`], manages worker tokens,
//! and tracks active [`ContainerHandle`] values through the in-memory registry.

use std::sync::Arc;

use tokio::sync::RwLock;
use uuid::Uuid;

use super::{
    CompletionResult, ContainerId, ContainerJobConfig, ContainerState, CreateJobParams, JobMode,
    JobRegistry, TokenStore,
};
use crate::error::OrchestratorError;
use crate::orchestrator::job_manager::docker_helpers::{
    ContainerConfigParams, append_claude_code_env, build_cmd, build_container_config,
    build_host_config, build_workspace_binds, create_and_start_container,
};

use crate::sandbox::connect_docker;
use crate::sandbox::container::DockerConnection;

/// Manages the lifecycle of Docker containers for sandboxed job execution.
pub struct ContainerJobManager {
    pub(super) config: ContainerJobConfig,
    pub(super) token_store: TokenStore,
    pub(crate) registry: JobRegistry,
    /// Cached Docker connection (created on first use).
    pub(super) docker: Arc<RwLock<Option<DockerConnection>>>,
}

impl ContainerJobManager {
    /// Create a Docker-backed job manager with an empty registry and lazy
    /// Docker connection cache.
    ///
    /// `config` controls container creation settings, and `token_store` owns
    /// the worker credentials minted for created jobs. The manager initializes
    /// a fresh [`JobRegistry`] and an empty `Arc<RwLock<Option<_>>>` cache that
    /// is populated on first Docker use.
    pub fn new(config: ContainerJobConfig, token_store: TokenStore) -> Self {
        Self {
            config,
            token_store,
            registry: JobRegistry::new(),
            docker: Arc::new(RwLock::new(None)),
        }
    }

    /// Get or create a Docker connection.
    async fn docker(&self) -> Result<DockerConnection, OrchestratorError> {
        if let Some(docker) = self.docker.read().await.clone() {
            return Ok(docker);
        }

        let docker = match connect_docker().await {
            Ok(docker) => docker,
            Err(e) => {
                return Err(OrchestratorError::Docker {
                    reason: e.to_string(),
                });
            }
        };

        let mut guard = self.docker.write().await;
        if let Some(existing) = guard.clone() {
            return Ok(existing);
        }
        *guard = Some(docker.clone());
        Ok(docker)
    }

    /// Inner implementation of container creation (separated for cleanup).
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
        let docker = self.docker().await?;

        let orchestrator_host = "host.docker.internal";

        let orchestrator_url = format!(
            "http://{}:{}",
            orchestrator_host, self.config.orchestrator_port
        );

        let mut env_vec = vec![
            format!("IRONCLAW_WORKER_TOKEN={}", token),
            format!("IRONCLAW_JOB_ID={}", job_id),
            format!("IRONCLAW_ORCHESTRATOR_URL={}", orchestrator_url),
        ];

        let binds = build_workspace_binds(project_dir.as_ref(), job_id, &mut env_vec).await?;

        if mode == JobMode::ClaudeCode {
            append_claude_code_env(&self.config, &mut env_vec);
        }

        let memory_mb = match mode {
            JobMode::ClaudeCode => self.config.claude_code_memory_limit_mb,
            JobMode::Worker => self.config.memory_limit_mb,
        };

        let host_config = build_host_config(binds, memory_mb, self.config.cpu_shares);
        let cmd = build_cmd(mode, job_id, &orchestrator_url, &self.config);

        let (container_config, options) = build_container_config(ContainerConfigParams {
            image: self.config.image.clone(),
            cmd,
            env_vec,
            host_config,
            job_id,
            mode,
        });

        let container_id =
            create_and_start_container(&docker, options, container_config, job_id).await?;

        self.registry
            .set_container_id(job_id, ContainerId::new(container_id))
            .await;

        tracing::info!(job_id = %job_id, "Created and started worker container");

        Ok(())
    }

    /// Stop and remove the container for `job_id`, then revoke its worker
    /// token and mark the handle as stopped.
    ///
    /// Returns an error when the job has no known container or Docker cannot
    /// be reached. Container stop and removal failures are logged as warnings
    /// and do not prevent the registry and token-store cleanup from running.
    pub async fn stop_job(&self, job_id: Uuid) -> Result<(), OrchestratorError> {
        let container_id = self
            .registry
            .container_id(job_id)
            .await
            .ok_or(OrchestratorError::ContainerNotFound { job_id })?;

        let docker = self.docker().await?;

        if let Err(e) = docker
            .stop_container(
                container_id.as_str(),
                Some(bollard::container::StopContainerOptions { t: 10 }),
            )
            .await
        {
            tracing::warn!(
                job_id = %job_id,
                error = %e,
                "Failed to stop container (may already be stopped)"
            );
        }

        if let Err(e) = docker
            .remove_container(
                container_id.as_str(),
                Some(bollard::container::RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await
        {
            tracing::warn!(
                job_id = %job_id,
                error = %e,
                "Failed to remove container (may require manual cleanup)"
            );
        }

        self.registry
            .set_state(job_id, ContainerState::Stopped)
            .await;

        self.token_store.revoke(job_id).await;

        tracing::info!(job_id = %job_id, "Stopped worker container");

        Ok(())
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
            match self.docker().await {
                Ok(docker) => {
                    if let Err(e) = docker
                        .stop_container(
                            container_id.as_str(),
                            Some(bollard::container::StopContainerOptions { t: 5 }),
                        )
                        .await
                    {
                        tracing::warn!(
                            job_id = %job_id,
                            error = %e,
                            "Failed to stop completed container"
                        );
                    }
                    if let Err(e) = docker
                        .remove_container(
                            container_id.as_str(),
                            Some(bollard::container::RemoveContainerOptions {
                                force: true,
                                ..Default::default()
                            }),
                        )
                        .await
                    {
                        tracing::warn!(
                            job_id = %job_id,
                            error = %e,
                            "Failed to remove completed container"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        job_id = %job_id,
                        error = %e,
                        "Failed to connect to Docker for container cleanup"
                    );
                }
            }
        }

        self.token_store.revoke(job_id).await;

        tracing::info!(job_id = %job_id, "Completed worker container");
        Ok(())
    }
}
