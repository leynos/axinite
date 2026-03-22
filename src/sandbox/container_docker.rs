use super::*;

use bollard::container::{
    Config, CreateContainerOptions, LogOutput, LogsOptions, WaitContainerOptions,
};
use bollard::exec::StartExecResults;
use bollard::models::HostConfig;
use futures::StreamExt;

#[cfg(feature = "docker")]
impl ContainerRunner {
    /// Create a container with the appropriate configuration.
    pub(super) async fn create_container(
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
    pub(super) async fn wait_for_container(
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
    pub(super) async fn run_exec(
        &self,
        exec_id: &str,
        max_output: usize,
    ) -> Result<ContainerOutput> {
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
