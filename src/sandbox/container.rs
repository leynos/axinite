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
#[cfg(all(unix, any(feature = "docker", test)))]
use std::path::PathBuf;
use std::time::Duration;

#[cfg(feature = "docker")]
use bollard::container::{
    Config, CreateContainerOptions, LogOutput, LogsOptions, RemoveContainerOptions,
    StartContainerOptions, WaitContainerOptions,
};
#[cfg(feature = "docker")]
use bollard::exec::{CreateExecOptions, StartExecResults};
#[cfg(feature = "docker")]
use bollard::models::HostConfig;
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

/// Connect to the Docker daemon.
///
/// When the crate is compiled without the `docker` feature, this returns
/// `SandboxError::DockerNotAvailable` immediately via
/// `docker_feature_disabled_error()`.
///
/// With the `docker` feature enabled, this delegates to
/// `connect_docker_inner()` and tries these locations in order:
///
/// Tries these locations in order:
/// 1. `DOCKER_HOST` env var (bollard default)
/// 2. `/var/run/docker.sock` (Linux default; also used by OrbStack and Podman Desktop on macOS)
/// 3. `~/.docker/run/docker.sock` (Docker Desktop 4.13+ on macOS — primary user-owned socket)
/// 4. `~/.colima/default/docker.sock` (Colima — popular lightweight Docker Desktop alternative)
/// 5. `~/.rd/docker.sock` (Rancher Desktop on macOS)
/// 6. `$XDG_RUNTIME_DIR/docker.sock` (common rootless Docker socket on Linux)
/// 7. `/run/user/$UID/docker.sock` (rootless Docker fallback on Linux)
pub async fn connect_docker() -> Result<DockerConnection> {
    #[cfg(feature = "docker")]
    {
        connect_docker_inner().await
    }

    #[cfg(not(feature = "docker"))]
    {
        Err(docker_feature_disabled_error())
    }
}

#[cfg(feature = "docker")]
async fn connect_docker_inner() -> Result<DockerConnection> {
    // First try bollard defaults (checks DOCKER_HOST env var, then /var/run/docker.sock).
    // This covers Linux, OrbStack (updates the /var/run symlink), and any user with
    // DOCKER_HOST set to their runtime's socket.
    if let Ok(docker) = DockerConnection::connect_with_local_defaults()
        && docker.ping().await.is_ok()
    {
        return Ok(docker);
    }

    #[cfg(unix)]
    {
        // Try well-known user-owned socket locations for desktop and rootless runtimes.
        // Docker Desktop 4.13+ (stabilised in 4.18) stopped creating the
        // /var/run/docker.sock symlink by default and moved the API socket
        // to ~/.docker/run/docker.sock.
        for sock in unix_socket_candidates() {
            if sock.exists() {
                let sock_str = sock.to_string_lossy();
                if let Ok(docker) = DockerConnection::connect_with_socket(
                    &sock_str,
                    120,
                    bollard::API_DEFAULT_VERSION,
                ) && docker.ping().await.is_ok()
                {
                    return Ok(docker);
                }
            }
        }
    }

    Err(SandboxError::DockerNotAvailable {
        reason: "Could not connect to Docker daemon. Tried: $DOCKER_HOST, \
            /var/run/docker.sock, ~/.docker/run/docker.sock, \
            ~/.colima/default/docker.sock, ~/.rd/docker.sock, \
            $XDG_RUNTIME_DIR/docker.sock, /run/user/$UID/docker.sock"
            .to_string(),
    })
}

#[cfg(not(feature = "docker"))]
pub(crate) fn docker_feature_disabled_error() -> SandboxError {
    SandboxError::DockerNotAvailable {
        reason: DOCKER_FEATURE_DISABLED_REASON.to_string(),
    }
}

#[cfg(all(unix, feature = "docker"))]
fn unix_socket_candidates() -> Vec<PathBuf> {
    unix_socket_candidates_from_env(
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
        std::env::var("UID").ok(),
    )
}

#[cfg(all(unix, any(feature = "docker", test)))]
fn unix_socket_candidates_from_env(
    home: Option<PathBuf>,
    xdg_runtime_dir: Option<PathBuf>,
    uid: Option<String>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut push_unique = |path: PathBuf| {
        if !candidates.iter().any(|existing| existing == &path) {
            candidates.push(path);
        }
    };

    if let Some(home) = home {
        push_unique(home.join(".docker/run/docker.sock")); // Docker Desktop 4.13+
        push_unique(home.join(".colima/default/docker.sock")); // Colima
        push_unique(home.join(".rd/docker.sock")); // Rancher Desktop
    }

    if let Some(xdg_runtime_dir) = xdg_runtime_dir {
        push_unique(xdg_runtime_dir.join("docker.sock"));
    }

    if let Some(uid) = uid.filter(|value| !value.is_empty()) {
        push_unique(PathBuf::from(format!("/run/user/{uid}/docker.sock")));
    }

    candidates
}

pub async fn docker_is_responsive(docker: &DockerConnection) -> bool {
    #[cfg(feature = "docker")]
    {
        docker.ping().await.is_ok()
    }

    #[cfg(not(feature = "docker"))]
    {
        let _ = docker;
        false
    }
}

pub async fn ensure_docker_responsive(docker: &DockerConnection) -> Result<()> {
    #[cfg(feature = "docker")]
    {
        docker
            .ping()
            .await
            .map(|_| ())
            .map_err(|e| SandboxError::DockerNotAvailable {
                reason: e.to_string(),
            })
    }

    #[cfg(not(feature = "docker"))]
    {
        let _ = docker;
        Err(docker_feature_disabled_error())
    }
}

#[cfg(feature = "docker")]
impl ContainerRunner {
    /// Create a container with the appropriate configuration.
    async fn create_container(
        &self,
        command: &str,
        working_dir: &Path,
        policy: SandboxPolicy,
        limits: &ResourceLimits,
        env: HashMap<String, String>,
    ) -> Result<String> {
        let working_dir_str = working_dir.display().to_string();

        let mut env_vec: Vec<String> = env
            .into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        let proxy_host = if cfg!(target_os = "linux") {
            "172.17.0.1"
        } else {
            "host.docker.internal"
        };

        if self.proxy_port > 0 && policy.is_sandboxed() {
            env_vec.push(format!(
                "http_proxy=http://{}:{}",
                proxy_host, self.proxy_port
            ));
            env_vec.push(format!(
                "https_proxy=http://{}:{}",
                proxy_host, self.proxy_port
            ));
            env_vec.push(format!(
                "HTTP_PROXY=http://{}:{}",
                proxy_host, self.proxy_port
            ));
            env_vec.push(format!(
                "HTTPS_PROXY=http://{}:{}",
                proxy_host, self.proxy_port
            ));
        }

        let binds = match policy {
            SandboxPolicy::ReadOnly => vec![format!("{}:/workspace:ro", working_dir_str)],
            SandboxPolicy::WorkspaceWrite => vec![format!("{}:/workspace:rw", working_dir_str)],
            SandboxPolicy::FullAccess => vec![
                format!("{}:/workspace:rw", working_dir_str),
                "/tmp:/tmp:rw".to_string(),
            ],
        };

        let host_config = HostConfig {
            binds: Some(binds),
            memory: Some((limits.memory_bytes) as i64),
            cpu_shares: Some(limits.cpu_shares as i64),
            auto_remove: Some(true),
            network_mode: Some("bridge".to_string()),
            cap_drop: Some(vec!["ALL".to_string()]),
            cap_add: Some(vec!["CHOWN".to_string()]),
            security_opt: Some(vec!["no-new-privileges:true".to_string()]),
            readonly_rootfs: Some(policy != SandboxPolicy::FullAccess),
            tmpfs: Some(
                [
                    ("/tmp".to_string(), "size=512M".to_string()),
                    (
                        "/home/sandbox/.cargo/registry".to_string(),
                        "size=1G".to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
            ),
            ..Default::default()
        };

        let config = Config {
            image: Some(self.image.clone()),
            cmd: Some(vec![
                "sh".to_string(),
                "-c".to_string(),
                command.to_string(),
            ]),
            working_dir: Some("/workspace".to_string()),
            env: Some(env_vec),
            host_config: Some(host_config),
            user: Some("1000:1000".to_string()),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: format!("sandbox-{}", uuid::Uuid::new_v4()),
            ..Default::default()
        };

        let response = self
            .docker
            .create_container(Some(options), config)
            .await
            .map_err(|e| SandboxError::ContainerCreationFailed {
                reason: e.to_string(),
            })?;

        Ok(response.id)
    }

    /// Wait for a container to complete and collect output.
    async fn wait_for_container(
        &self,
        container_id: &str,
        max_output: usize,
    ) -> Result<ContainerOutput> {
        let mut wait_stream = self.docker.wait_container(
            container_id,
            Some(WaitContainerOptions {
                condition: "not-running",
            }),
        );

        let exit_code = match wait_stream.next().await {
            Some(Ok(response)) => response.status_code,
            Some(Err(e)) => {
                return Err(SandboxError::ExecutionFailed {
                    reason: format!("wait failed: {}", e),
                });
            }
            None => {
                return Err(SandboxError::ExecutionFailed {
                    reason: "container wait stream ended unexpectedly".to_string(),
                });
            }
        };

        let (stdout, stderr, truncated) = self.collect_logs(container_id, max_output).await?;

        Ok(ContainerOutput {
            exit_code,
            stdout,
            stderr,
            duration: Duration::ZERO,
            truncated,
        })
    }

    /// Collect stdout and stderr from a container.
    async fn collect_logs(
        &self,
        container_id: &str,
        max_output: usize,
    ) -> Result<(String, String, bool)> {
        let options = LogsOptions::<String> {
            stdout: true,
            stderr: true,
            follow: false,
            ..Default::default()
        };

        let mut stream = self.docker.logs(container_id, Some(options));

        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut truncated = false;
        let half_max = max_output / 2;

        while let Some(result) = stream.next().await {
            match result {
                Ok(LogOutput::StdOut { message }) => {
                    let text = String::from_utf8_lossy(&message);
                    truncated |= append_with_limit(&mut stdout, &text, half_max);
                }
                Ok(LogOutput::StdErr { message }) => {
                    let text = String::from_utf8_lossy(&message);
                    truncated |= append_with_limit(&mut stderr, &text, half_max);
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("Error reading container logs: {}", e);
                }
            }
        }

        Ok((stdout, stderr, truncated))
    }

    /// Run an exec and collect output.
    async fn run_exec(&self, exec_id: &str, max_output: usize) -> Result<ContainerOutput> {
        let start_result = self.docker.start_exec(exec_id, None).await.map_err(|e| {
            SandboxError::ExecutionFailed {
                reason: format!("exec start failed: {}", e),
            }
        })?;

        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut truncated = false;
        let half_max = max_output / 2;

        if let StartExecResults::Attached { mut output, .. } = start_result {
            while let Some(result) = output.next().await {
                match result {
                    Ok(LogOutput::StdOut { message }) => {
                        let text = String::from_utf8_lossy(&message);
                        truncated |= append_with_limit(&mut stdout, &text, half_max);
                    }
                    Ok(LogOutput::StdErr { message }) => {
                        let text = String::from_utf8_lossy(&message);
                        truncated |= append_with_limit(&mut stderr, &text, half_max);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("Error reading exec output: {}", e);
                    }
                }
            }
        }

        let inspect =
            self.docker
                .inspect_exec(exec_id)
                .await
                .map_err(|e| SandboxError::ExecutionFailed {
                    reason: format!("exec inspect failed: {}", e),
                })?;

        let exit_code = inspect.exit_code.unwrap_or(-1);

        Ok(ContainerOutput {
            exit_code,
            stdout,
            stderr,
            duration: Duration::ZERO,
            truncated,
        })
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

    #[cfg(unix)]
    #[test]
    fn test_unix_socket_candidates_include_rootless_paths() {
        let candidates = unix_socket_candidates_from_env(
            Some(PathBuf::from("/home/tester")),
            Some(PathBuf::from("/run/user/1000")),
            Some("1000".to_string()),
        );

        assert!(candidates.contains(&PathBuf::from("/home/tester/.docker/run/docker.sock")));
        assert!(candidates.contains(&PathBuf::from("/home/tester/.colima/default/docker.sock")));
        assert!(candidates.contains(&PathBuf::from("/home/tester/.rd/docker.sock")));
        assert!(candidates.contains(&PathBuf::from("/run/user/1000/docker.sock")));
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

        let docker = result.unwrap();
        let runner = ContainerRunner::new(docker, "alpine:latest".to_string(), 0);
        // Just check that we can query Docker (result doesn't matter for CI)
        let _available = runner.is_available().await;
    }
}
