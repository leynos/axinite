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

/// Build bind mounts and related environment for a job workspace.
async fn build_workspace_binds(
    project_dir: Option<&PathBuf>,
    job_id: Uuid,
    env_vec: &mut Vec<String>,
) -> Result<Vec<String>, OrchestratorError> {
    let mut binds = Vec::new();
    if let Some(dir) = project_dir {
        let canonical = bind_mount::validate_bind_mount_path(dir, job_id).await?;
        binds.push(format!("{}:/workspace:rw", canonical.display()));
        env_vec.push("IRONCLAW_WORKSPACE=/workspace".to_string());
    }

    Ok(binds)
}

/// Append Claude Code-specific environment variables.
fn append_claude_code_env(config: &ContainerJobConfig, env_vec: &mut Vec<String>) {
    if let Some(ref api_key) = config.claude_code_api_key {
        env_vec.push(format!("ANTHROPIC_API_KEY={}", api_key));
    } else if let Some(ref oauth_token) = config.claude_code_oauth_token {
        env_vec.push(format!("CLAUDE_CODE_OAUTH_TOKEN={}", oauth_token));
    }
    if !config.claude_code_allowed_tools.is_empty() {
        env_vec.push(format!(
            "CLAUDE_CODE_ALLOWED_TOOLS={}",
            config.claude_code_allowed_tools.join(",")
        ));
    }
}

/// Build the Docker host configuration for a job container.
fn build_host_config(
    binds: Vec<String>,
    memory_mb: u64,
    cpu_shares: u32,
) -> bollard::models::HostConfig {
    use bollard::models::HostConfig;

    HostConfig {
        binds: if binds.is_empty() { None } else { Some(binds) },
        memory: Some((memory_mb * 1024 * 1024) as i64),
        cpu_shares: Some(cpu_shares as i64),
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
    }
}

/// Build the container command for a job mode.
fn build_cmd(
    mode: JobMode,
    job_id: Uuid,
    orchestrator_url: &str,
    config: &ContainerJobConfig,
) -> Vec<String> {
    match mode {
        JobMode::Worker => vec![
            "worker".to_string(),
            "--job-id".to_string(),
            job_id.to_string(),
            "--orchestrator-url".to_string(),
            orchestrator_url.to_string(),
        ],
        JobMode::ClaudeCode => vec![
            "claude-bridge".to_string(),
            "--job-id".to_string(),
            job_id.to_string(),
            "--orchestrator-url".to_string(),
            orchestrator_url.to_string(),
            "--max-turns".to_string(),
            config.claude_code_max_turns.to_string(),
            "--model".to_string(),
            config.claude_code_model.clone(),
        ],
    }
}

/// Build the Docker container name for a job mode.
fn container_name(mode: JobMode, job_id: Uuid) -> String {
    match mode {
        JobMode::Worker => format!("ironclaw-worker-{}", job_id),
        JobMode::ClaudeCode => format!("ironclaw-claude-{}", job_id),
    }
}

/// Build the Docker container configuration and options for a job.
fn build_container_config(
    image: &str,
    cmd: Vec<String>,
    env_vec: Vec<String>,
    host_config: bollard::models::HostConfig,
    job_id: Uuid,
    mode: JobMode,
) -> (
    bollard::container::Config<String>,
    bollard::container::CreateContainerOptions<String>,
) {
    let mut labels = std::collections::HashMap::new();
    labels.insert("ironclaw.job_id".to_string(), job_id.to_string());
    labels.insert(
        "ironclaw.created_at".to_string(),
        chrono::Utc::now().to_rfc3339(),
    );

    let config = bollard::container::Config {
        image: Some(image.to_string()),
        cmd: Some(cmd),
        env: Some(env_vec),
        host_config: Some(host_config),
        user: Some("1000:1000".to_string()),
        working_dir: Some("/workspace".to_string()),
        labels: Some(labels),
        ..Default::default()
    };

    let options = bollard::container::CreateContainerOptions {
        name: container_name(mode, job_id),
        ..Default::default()
    };

    (config, options)
}

/// Create and start a Docker container, cleaning up if start fails.
async fn create_and_start_container(
    docker: &DockerConnection,
    options: bollard::container::CreateContainerOptions<String>,
    config: bollard::container::Config<String>,
    job_id: Uuid,
) -> Result<String, OrchestratorError> {
    let response = docker
        .create_container(Some(options), config)
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

    Ok(container_id)
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
                if let Some(docker) = self.docker.read().await.clone() {
                    return Ok(docker);
                }
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

        let (container_config, options) =
            build_container_config(&self.config.image, cmd, env_vec, host_config, job_id, mode);

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
