//! Docker container lifecycle management.
//!
//! Handles creating, running, and cleaning up containers for sandboxed execution.
//!
//! # Container Setup
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────────┐
//! │                          Docker Container                               │
//! │                                                                         │
//! │  Environment:                                                           │
//! │    http_proxy=http://host.docker.internal:PORT                          │
//! │    https_proxy=http://host.docker.internal:PORT                         │
//! │    (No secrets or credentials)                                          │
//! │                                                                         │
//! │  Mounts:                                                                │
//! │    /workspace ─▶ Host working directory (ro or rw based on policy)     │
//! │    /output    ─▶ Output directory for artifacts (rw)                   │
//! │                                                                         │
//! │  Limits:                                                                │
//! │    Memory: 2GB (default)                                                │
//! │    CPU: 1024 shares                                                     │
//! │    No privileged mode                                                   │
//! │    Non-root user (UID 1000)                                             │
//! └────────────────────────────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

#[cfg(feature = "docker")]
use bollard::container::{RemoveContainerOptions, StartContainerOptions};
#[cfg(feature = "docker")]
use bollard::exec::CreateExecOptions;
#[cfg(feature = "docker")]
use futures::StreamExt;

use crate::sandbox::config::{ResourceLimits, SandboxPolicy};
use crate::sandbox::error::{Result, SandboxError};

#[cfg(feature = "docker")]
pub type DockerConnection = bollard::Docker;

#[cfg(not(feature = "docker"))]
#[derive(Debug, Clone, Default)]
pub struct DockerConnection;

#[cfg(not(feature = "docker"))]
pub(crate) const DOCKER_FEATURE_DISABLED_REASON: &str =
    "Docker support was not compiled in; rebuild with --features docker";

#[path = "container_connection.rs"]
mod container_connection;

#[cfg(feature = "docker")]
#[path = "container_docker.rs"]
mod container_docker;

#[cfg(not(feature = "docker"))]
pub(crate) use self::container_connection::docker_feature_disabled_error;
pub use self::container_connection::{
    connect_docker, docker_is_responsive, ensure_docker_responsive,
};

/// Output from container execution.
#[derive(Debug, Clone)]
pub struct ContainerOutput {
    /// Exit code from the command.
    pub exit_code: i64,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
    /// How long the command ran.
    pub duration: Duration,
    /// Whether output was truncated.
    pub truncated: bool,
}

/// Manages Docker container lifecycle.
pub struct ContainerRunner {
    docker: DockerConnection,
    image: String,
    proxy_port: u16,
}

/// Append `text` into `buffer` up to `limit` bytes without breaking UTF-8.
///
/// Returns `true` when truncation occurred.
#[cfg(any(feature = "docker", test))]
fn append_with_limit(buffer: &mut String, text: &str, limit: usize) -> bool {
    if text.is_empty() {
        return false;
    }

    if buffer.len() >= limit {
        return true;
    }

    let remaining = limit - buffer.len();
    if text.len() <= remaining {
        buffer.push_str(text);
        return false;
    }

    let end = crate::util::floor_char_boundary(text, remaining);
    buffer.push_str(&text[..end]);
    true
}

impl ContainerRunner {
    /// Create a new container runner.
    pub fn new(docker: DockerConnection, image: String, proxy_port: u16) -> Self {
        Self {
            docker,
            image,
            proxy_port,
        }
    }

    /// Check if the Docker daemon is available.
    pub async fn is_available(&self) -> bool {
        docker_is_responsive(&self.docker).await
    }

    /// Check if the sandbox image exists locally.
    pub async fn image_exists(&self) -> bool {
        #[cfg(feature = "docker")]
        {
            self.docker.inspect_image(&self.image).await.is_ok()
        }

        #[cfg(not(feature = "docker"))]
        {
            false
        }
    }

    /// Pull the sandbox image.
    pub async fn pull_image(&self) -> Result<()> {
        #[cfg(feature = "docker")]
        {
            use bollard::image::CreateImageOptions;

            tracing::info!("Pulling sandbox image: {}", self.image);

            let options = CreateImageOptions {
                from_image: self.image.clone(),
                ..Default::default()
            };

            let mut stream = self.docker.create_image(Some(options), None, None);

            while let Some(result) = stream.next().await {
                match result {
                    Ok(info) => {
                        if let Some(status) = info.status {
                            tracing::debug!("Pull status: {}", status);
                        }
                    }
                    Err(e) => {
                        return Err(SandboxError::ContainerCreationFailed {
                            reason: format!("image pull failed: {}", e),
                        });
                    }
                }
            }

            tracing::info!("Successfully pulled image: {}", self.image);
            Ok(())
        }

        #[cfg(not(feature = "docker"))]
        {
            let _ = (&self.image, self.proxy_port);
            Err(docker_feature_disabled_error())
        }
    }

    /// Execute a command in a new container.
    pub async fn execute(
        &self,
        command: &str,
        working_dir: &Path,
        policy: SandboxPolicy,
        limits: &ResourceLimits,
        env: HashMap<String, String>,
    ) -> Result<ContainerOutput> {
        #[cfg(feature = "docker")]
        {
            let start_time = std::time::Instant::now();

            let container_id = self
                .create_container(command, working_dir, policy, limits, env)
                .await?;

            if let Err(e) = self
                .docker
                .start_container(&container_id, None::<StartContainerOptions<String>>)
                .await
            {
                let _ = self
                    .docker
                    .remove_container(
                        &container_id,
                        Some(RemoveContainerOptions {
                            force: true,
                            ..Default::default()
                        }),
                    )
                    .await;

                return Err(SandboxError::ContainerStartFailed {
                    reason: e.to_string(),
                });
            }

            let result = tokio::time::timeout(limits.timeout, async {
                self.wait_for_container(&container_id, limits.max_output_bytes)
                    .await
            })
            .await;

            let _ = self
                .docker
                .remove_container(
                    &container_id,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await;

            match result {
                Ok(Ok(mut output)) => {
                    output.duration = start_time.elapsed();
                    Ok(output)
                }
                Ok(Err(e)) => Err(e),
                Err(_) => Err(SandboxError::Timeout(limits.timeout)),
            }
        }

        #[cfg(not(feature = "docker"))]
        {
            let _ = (
                command,
                working_dir,
                policy,
                limits,
                env,
                &self.image,
                self.proxy_port,
            );
            Err(docker_feature_disabled_error())
        }
    }

    /// Execute a command in an existing container using exec.
    pub async fn exec_in_container(
        &self,
        container_id: &str,
        command: &str,
        working_dir: &str,
        limits: &ResourceLimits,
    ) -> Result<ContainerOutput> {
        #[cfg(feature = "docker")]
        {
            let start_time = std::time::Instant::now();

            let exec = self
                .docker
                .create_exec(
                    container_id,
                    CreateExecOptions {
                        cmd: Some(vec!["sh", "-c", command]),
                        attach_stdout: Some(true),
                        attach_stderr: Some(true),
                        working_dir: Some(working_dir),
                        ..Default::default()
                    },
                )
                .await
                .map_err(|e| SandboxError::ExecutionFailed {
                    reason: format!("exec create failed: {}", e),
                })?;

            let result = tokio::time::timeout(
                limits.timeout,
                self.run_exec(&exec.id, limits.max_output_bytes),
            )
            .await;

            match result {
                Ok(Ok(mut output)) => {
                    output.duration = start_time.elapsed();
                    Ok(output)
                }
                Ok(Err(e)) => Err(e),
                Err(_) => Err(SandboxError::Timeout(limits.timeout)),
            }
        }

        #[cfg(not(feature = "docker"))]
        {
            let _ = (
                container_id,
                command,
                working_dir,
                limits,
                &self.image,
                self.proxy_port,
            );
            Err(docker_feature_disabled_error())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_with_limit_truncates_on_utf8_boundary() {
        let mut out = String::new();
        let truncated = append_with_limit(&mut out, "ab🙂cd", 5);
        assert!(truncated);
        assert_eq!(out, "ab");
    }

    #[test]
    fn append_with_limit_marks_truncated_when_full() {
        let mut out = "abc".to_string();
        let truncated = append_with_limit(&mut out, "z", 3);
        assert!(truncated);
        assert_eq!(out, "abc");
    }

    #[test]
    fn append_with_limit_appends_without_truncation() {
        let mut out = String::new();
        let truncated = append_with_limit(&mut out, "hello", 10);
        assert!(!truncated);
        assert_eq!(out, "hello");
    }

    #[cfg(feature = "docker")]
    #[tokio::test]
    async fn test_docker_connection() {
        // This test requires Docker to be running
        let result = connect_docker().await;
        // Don't fail if Docker isn't available, just skip
        if result.is_err() {
            eprintln!("Skipping Docker test: Docker not available");
            return;
        }

        let docker = result.expect("failed to create docker client");
        let runner = ContainerRunner::new(docker, "alpine:latest".to_string(), 0);
        // Just check that we can query Docker (result doesn't matter for CI)
        let _available = runner.is_available().await;
    }
}
