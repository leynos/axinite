//! Container lifecycle management for sandboxed jobs.
//!
//! Extends the existing `SandboxManager` infrastructure to support persistent
//! containers with their own agent loops (as opposed to ephemeral per-command containers).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use uuid::Uuid;

#[cfg(any(feature = "docker", test))]
use crate::bootstrap::ironclaw_base_dir;
use crate::error::OrchestratorError;
use crate::orchestrator::auth::{CredentialGrant, TokenStore};

/// Which mode a sandbox container runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobMode {
    /// Standard IronClaw worker with proxied LLM calls.
    Worker,
    /// Claude Code bridge that spawns the `claude` CLI directly.
    ClaudeCode,
}

impl JobMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::ClaudeCode => "claude_code",
        }
    }
}

impl std::fmt::Display for JobMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Configuration for the container job manager.
#[derive(Debug, Clone)]
pub struct ContainerJobConfig {
    /// Docker image for worker containers.
    pub image: String,
    /// Default memory limit in MB.
    pub memory_limit_mb: u64,
    /// Default CPU shares.
    pub cpu_shares: u32,
    /// Port the orchestrator internal API listens on.
    pub orchestrator_port: u16,
    /// Anthropic API key for Claude Code containers (read from ANTHROPIC_API_KEY).
    /// Takes priority over OAuth token.
    pub claude_code_api_key: Option<String>,
    /// OAuth access token extracted from the host's `claude login` session.
    /// Passed as CLAUDE_CODE_OAUTH_TOKEN to containers. Falls back to this
    /// when no ANTHROPIC_API_KEY is available.
    pub claude_code_oauth_token: Option<String>,
    /// Claude model to use in ClaudeCode mode.
    pub claude_code_model: String,
    /// Maximum turns for Claude Code.
    pub claude_code_max_turns: u32,
    /// Memory limit in MB for Claude Code containers (heavier than workers).
    pub claude_code_memory_limit_mb: u64,
    /// Allowed tool patterns for Claude Code (passed as CLAUDE_CODE_ALLOWED_TOOLS env var).
    pub claude_code_allowed_tools: Vec<String>,
}

impl Default for ContainerJobConfig {
    fn default() -> Self {
        Self {
            image: "ironclaw-worker:latest".to_string(),
            memory_limit_mb: 2048,
            cpu_shares: 1024,
            orchestrator_port: 50051,
            claude_code_api_key: None,
            claude_code_oauth_token: None,
            claude_code_model: "sonnet".to_string(),
            claude_code_max_turns: 50,
            claude_code_memory_limit_mb: 4096,
            claude_code_allowed_tools: crate::config::ClaudeCodeConfig::default().allowed_tools,
        }
    }
}

/// State of a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    Creating,
    Running,
    Stopped,
    Failed,
}

impl std::fmt::Display for ContainerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Creating => write!(f, "creating"),
            Self::Running => write!(f, "running"),
            Self::Stopped => write!(f, "stopped"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// Handle to a running container job.
#[derive(Debug, Clone)]
pub struct ContainerHandle {
    pub job_id: Uuid,
    pub container_id: String,
    pub state: ContainerState,
    pub mode: JobMode,
    pub created_at: DateTime<Utc>,
    pub project_dir: Option<PathBuf>,
    pub task_description: String,
    /// Last status message reported by the worker (iteration count, progress, etc.).
    pub last_worker_status: Option<String>,
    /// Which iteration the worker is on (updated via status reports).
    pub worker_iteration: u32,
    /// Completion result from the worker (set when the worker reports done).
    pub completion_result: Option<CompletionResult>,
    // NOTE: auth_token is intentionally NOT in this struct.
    // It lives only in the TokenStore (never logged, serialized, or persisted).
}

/// Result reported by a worker on completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionResult {
    pub success: bool,
    pub message: Option<String>,
}

/// Validate that a project directory is under `~/.ironclaw/projects/`.
///
/// Returns the canonicalized path if valid. Creates the base directory if
/// it doesn't exist (so the prefix check always runs).
///
/// # TOCTOU note
///
/// There is a time-of-check/time-of-use gap between `canonicalize()` here
/// and the actual Docker `binds.push()` in the caller. In a multi-tenant
/// system a malicious actor could swap a symlink after validation. This is
/// acceptable in IronClaw's single-tenant design where the user controls
/// the filesystem.
#[cfg(any(feature = "docker", test))]
fn validate_bind_mount_path(
    dir: &std::path::Path,
    job_id: Uuid,
) -> Result<PathBuf, OrchestratorError> {
    let canonical = dir
        .canonicalize()
        .map_err(|e| OrchestratorError::ContainerCreationFailed {
            job_id,
            reason: format!(
                "failed to canonicalize project dir {}: {}",
                dir.display(),
                e
            ),
        })?;

    let projects_base = ironclaw_base_dir().join("projects");

    if !projects_base.is_absolute() {
        return Err(OrchestratorError::ContainerCreationFailed {
            job_id,
            reason: "base directory is not absolute; cannot safely validate bind mounts".into(),
        });
    }

    // Ensure the base exists so canonicalize always succeeds.
    std::fs::create_dir_all(&projects_base).map_err(|e| {
        OrchestratorError::ContainerCreationFailed {
            job_id,
            reason: format!(
                "failed to create projects base {}: {}",
                projects_base.display(),
                e
            ),
        }
    })?;

    let canonical_base =
        projects_base
            .canonicalize()
            .map_err(|e| OrchestratorError::ContainerCreationFailed {
                job_id,
                reason: format!(
                    "failed to canonicalize projects base {}: {}",
                    projects_base.display(),
                    e
                ),
            })?;

    if !canonical.starts_with(&canonical_base) {
        return Err(OrchestratorError::ContainerCreationFailed {
            job_id,
            reason: format!(
                "project directory {} is outside allowed base {}",
                canonical.display(),
                canonical_base.display()
            ),
        });
    }

    Ok(canonical)
}

#[cfg(feature = "docker")]
mod docker;
#[cfg(not(feature = "docker"))]
mod no_docker;
#[cfg(test)]
mod tests;

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
            created_at: Utc::now(),
            project_dir: project_dir.clone(),
            task_description: task.to_string(),
            last_worker_status: None,
            worker_iteration: 0,
            completion_result: None,
        };
        self.containers.write().await.insert(job_id, handle);

        // Run the actual container creation. On any failure, revoke the token
        // and remove the handle so we don't leak resources.
        match self
            .create_job_inner(job_id, &token, project_dir, mode)
            .await
        {
            Ok(()) => Ok(token),
            Err(e) => {
                self.token_store.revoke(job_id).await;
                self.containers.write().await.remove(&job_id);
                Err(e)
            }
        }
    }
}

impl ContainerJobManager {
    /// Remove a completed job handle from memory (called after result is read).
    pub async fn cleanup_job(&self, job_id: Uuid) {
        self.containers.write().await.remove(&job_id);
    }

    /// Update the worker-reported status for a job.
    pub async fn update_worker_status(
        &self,
        job_id: Uuid,
        message: Option<String>,
        iteration: u32,
    ) {
        if let Some(handle) = self.containers.write().await.get_mut(&job_id) {
            handle.last_worker_status = message;
            handle.worker_iteration = iteration;
        }
    }

    /// Get the handle for a job.
    pub async fn get_handle(&self, job_id: Uuid) -> Option<ContainerHandle> {
        self.containers.read().await.get(&job_id).cloned()
    }

    /// List all active container jobs.
    pub async fn list_jobs(&self) -> Vec<ContainerHandle> {
        self.containers.read().await.values().cloned().collect()
    }

    /// Get a reference to the token store.
    pub fn token_store(&self) -> &TokenStore {
        &self.token_store
    }
}
