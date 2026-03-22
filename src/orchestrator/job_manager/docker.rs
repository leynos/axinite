//! Docker-backed container job orchestration.
//!
//! This module implements [`ContainerJobManager`] for sandboxed job execution
//! when Docker support is enabled. It caches a [`DockerConnection`] from
//! `connect_docker()`, applies [`ContainerJobConfig`], manages worker tokens,
//! and tracks active [`ContainerHandle`] values through the in-memory registry.

use super::*;

use crate::orchestrator::bind_mount;
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
        {
            let guard = self.docker.read().await;
            if let Some(ref docker) = *guard {
                return Ok(docker.clone());
            }
        }

        let docker = connect_docker()
            .await
            .map_err(|e| OrchestratorError::Docker {
                reason: e.to_string(),
            })?;
        *self.docker.write().await = Some(docker.clone());
        Ok(docker)
    }

    /// Inner implementation of container creation (separated for cleanup).
    pub(super) async fn create_job_inner(
        &self,
        job_id: Uuid,
        token: &str,
        project_dir: Option<PathBuf>,
        mode: JobMode,
    ) -> Result<(), OrchestratorError> {
        let docker = self.docker().await?;

        let orchestrator_host = if cfg!(target_os = "linux") {
            "172.17.0.1"
        } else {
            "host.docker.internal"
        };

        let orchestrator_url = format!(
            "http://{}:{}",
            orchestrator_host, self.config.orchestrator_port
        );

        let mut env_vec = vec![
            format!("IRONCLAW_WORKER_TOKEN={}", token),
            format!("IRONCLAW_JOB_ID={}", job_id),
            format!("IRONCLAW_ORCHESTRATOR_URL={}", orchestrator_url),
        ];

        let mut binds = Vec::new();
        if let Some(ref dir) = project_dir {
            let canonical = bind_mount::validate_bind_mount_path(dir, job_id)?;
            binds.push(format!("{}:/workspace:rw", canonical.display()));
            env_vec.push("IRONCLAW_WORKSPACE=/workspace".to_string());
        }

        if mode == JobMode::ClaudeCode {
            if let Some(ref api_key) = self.config.claude_code_api_key {
                env_vec.push(format!("ANTHROPIC_API_KEY={}", api_key));
            } else if let Some(ref oauth_token) = self.config.claude_code_oauth_token {
                env_vec.push(format!("CLAUDE_CODE_OAUTH_TOKEN={}", oauth_token));
            }
            if !self.config.claude_code_allowed_tools.is_empty() {
                env_vec.push(format!(
                    "CLAUDE_CODE_ALLOWED_TOOLS={}",
                    self.config.claude_code_allowed_tools.join(",")
                ));
            }
        }

        let memory_mb = match mode {
            JobMode::ClaudeCode => self.config.claude_code_memory_limit_mb,
            JobMode::Worker => self.config.memory_limit_mb,
        };

        use bollard::container::{Config, CreateContainerOptions};
        use bollard::models::HostConfig;

        let host_config = HostConfig {
            binds: if binds.is_empty() { None } else { Some(binds) },
            memory: Some((memory_mb * 1024 * 1024) as i64),
            cpu_shares: Some(self.config.cpu_shares as i64),
            network_mode: Some("bridge".to_string()),
            extra_hosts: Some(vec!["host.docker.internal:host-gateway".to_string()]),
            cap_drop: Some(vec!["ALL".to_string()]),
            cap_add: Some(vec!["CHOWN".to_string()]),
            security_opt: Some(vec!["no-new-privileges:true".to_string()]),
            tmpfs: Some(
                [("/tmp".to_string(), "size=512M".to_string())]
                    .into_iter()
                    .collect(),
            ),
            ..Default::default()
        };

        let cmd = match mode {
            JobMode::Worker => vec![
                "worker".to_string(),
                "--job-id".to_string(),
                job_id.to_string(),
                "--orchestrator-url".to_string(),
                orchestrator_url,
            ],
            JobMode::ClaudeCode => vec![
                "claude-bridge".to_string(),
                "--job-id".to_string(),
                job_id.to_string(),
                "--orchestrator-url".to_string(),
                orchestrator_url,
                "--max-turns".to_string(),
                self.config.claude_code_max_turns.to_string(),
                "--model".to_string(),
                self.config.claude_code_model.clone(),
            ],
        };

        let mut labels = std::collections::HashMap::new();
        labels.insert("ironclaw.job_id".to_string(), job_id.to_string());
        labels.insert(
            "ironclaw.created_at".to_string(),
            chrono::Utc::now().to_rfc3339(),
        );

        let container_config = Config {
            image: Some(self.config.image.clone()),
            cmd: Some(cmd),
            env: Some(env_vec),
            host_config: Some(host_config),
            user: Some("1000:1000".to_string()),
            working_dir: Some("/workspace".to_string()),
            labels: Some(labels),
            ..Default::default()
        };

        let container_name = match mode {
            JobMode::Worker => format!("ironclaw-worker-{}", job_id),
            JobMode::ClaudeCode => format!("ironclaw-claude-{}", job_id),
        };
        let options = CreateContainerOptions {
            name: container_name,
            ..Default::default()
        };

        let response = docker
            .create_container(Some(options), container_config)
            .await
            .map_err(|e| OrchestratorError::ContainerCreationFailed {
                job_id,
                reason: e.to_string(),
            })?;

        let container_id = response.id;

        if let Err(e) = docker.start_container::<String>(&container_id, None).await {
            if let Err(remove_error) = docker
                .remove_container(
                    &container_id,
                    Some(bollard::container::RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await
            {
                tracing::warn!(
                    job_id = %job_id,
                    container_id = %container_id,
                    error = %remove_error,
                    "Failed to remove container after start failure"
                );
            }

            return Err(OrchestratorError::ContainerCreationFailed {
                job_id,
                reason: format!("failed to start container: {}", e),
            });
        }

        self.registry.set_container_id(job_id, container_id).await;

        tracing::info!(job_id = %job_id, "Created and started worker container");

        Ok(())
    }

    pub async fn stop_job(&self, job_id: Uuid) -> Result<(), OrchestratorError> {
        let container_id = self
            .registry
            .container_id(job_id)
            .await
            .ok_or(OrchestratorError::ContainerNotFound { job_id })?;

        if container_id.is_empty() {
            return Err(OrchestratorError::InvalidContainerState {
                job_id,
                state: "creating (no container ID yet)".to_string(),
            });
        }

        let docker = self.docker().await?;

        if let Err(e) = docker
            .stop_container(
                &container_id,
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
                &container_id,
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

        if let Some(container_id) = self
            .registry
            .container_id(job_id)
            .await
            .filter(|container_id| !container_id.is_empty())
        {
            match self.docker().await {
                Ok(docker) => {
                    if let Err(e) = docker
                        .stop_container(
                            &container_id,
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
                            &container_id,
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
