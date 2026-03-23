//! Helper functions for Docker-backed container job orchestration.
//!
//! This module keeps container configuration assembly and workspace-binding
//! setup separate from the higher-level job lifecycle methods.

use std::path::PathBuf;

use bollard::container::{Config, CreateContainerOptions, RemoveContainerOptions};
use chrono::Utc;
use uuid::Uuid;

use crate::error::OrchestratorError;
use crate::orchestrator::bind_mount;
use crate::orchestrator::job_manager::{ContainerJobConfig, JobMode};
use crate::sandbox::container::DockerConnection;

/// Build bind mounts and related environment for a job workspace.
pub(super) async fn build_workspace_binds(
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
pub(super) fn append_claude_code_env(config: &ContainerJobConfig, env_vec: &mut Vec<String>) {
    if config.claude_code_api_key.is_some() && config.claude_code_oauth_token.is_some() {
        tracing::warn!(
            "Both claude_code_api_key and claude_code_oauth_token are set; using the API key"
        );
    } else if config.claude_code_oauth_token.is_some() {
        tracing::info!("Using CLAUDE_CODE_OAUTH_TOKEN for Claude Code authentication");
    }

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
pub(super) fn build_host_config(
    binds: Vec<String>,
    memory_mb: u64,
    cpu_shares: u32,
) -> bollard::models::HostConfig {
    use bollard::models::HostConfig;

    let memory_bytes = memory_mb
        .saturating_mul(1024)
        .saturating_mul(1024)
        .min(i64::MAX as u64) as i64;

    HostConfig {
        binds: if binds.is_empty() { None } else { Some(binds) },
        memory: Some(memory_bytes),
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
pub(super) fn build_cmd(
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
pub(super) fn container_name(mode: JobMode, job_id: Uuid) -> String {
    match mode {
        JobMode::Worker => format!("ironclaw-worker-{}", job_id),
        JobMode::ClaudeCode => format!("ironclaw-claude-{}", job_id),
    }
}

/// Inputs used to assemble the Docker container configuration for a job.
pub(super) struct ContainerConfigParams {
    pub(super) image: String,
    pub(super) cmd: Vec<String>,
    pub(super) env_vec: Vec<String>,
    pub(super) host_config: bollard::models::HostConfig,
    pub(super) job_id: Uuid,
    pub(super) mode: JobMode,
}

/// Build the Docker container configuration and options for a job.
pub(super) fn build_container_config(
    params: ContainerConfigParams,
) -> (Config<String>, CreateContainerOptions<String>) {
    let ContainerConfigParams {
        image,
        cmd,
        env_vec,
        host_config,
        job_id,
        mode,
    } = params;

    let mut labels = std::collections::HashMap::new();
    labels.insert("ironclaw.job_id".to_string(), job_id.to_string());
    labels.insert("ironclaw.created_at".to_string(), Utc::now().to_rfc3339());

    let config = Config {
        image: Some(image),
        cmd: Some(cmd),
        env: Some(env_vec),
        host_config: Some(host_config),
        user: Some("1000:1000".to_string()),
        working_dir: Some("/workspace".to_string()),
        labels: Some(labels),
        ..Default::default()
    };

    let options = CreateContainerOptions {
        name: container_name(mode, job_id),
        ..Default::default()
    };

    (config, options)
}

/// Create and start a Docker container, cleaning up if start fails.
pub(super) async fn create_and_start_container(
    docker: &DockerConnection,
    options: CreateContainerOptions<String>,
    config: Config<String>,
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
                Some(RemoveContainerOptions {
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
