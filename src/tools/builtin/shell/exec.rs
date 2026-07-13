//! Command execution paths for `ShellTool`: sandboxed (Docker) execution,
//! direct host execution with environment scrubbing, and the shared
//! validation-then-dispatch entry point.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::sandbox::SandboxManager;
use crate::tools::tool::ToolError;

use super::detect_command_injection;
use super::policy::{MAX_OUTPUT_SIZE, SAFE_ENV_VARS};
use super::tool::ShellTool;

impl ShellTool {
    /// Execute a command through the sandbox.
    async fn execute_sandboxed(
        &self,
        sandbox: &SandboxManager,
        cmd: &str,
        workdir: &Path,
        timeout: Duration,
    ) -> Result<(String, i64), ToolError> {
        // Override sandbox config timeout if needed
        let result = tokio::time::timeout(timeout, async {
            sandbox
                .execute_with_policy(
                    cmd,
                    workdir,
                    self.sandbox_policy,
                    std::collections::HashMap::new(),
                )
                .await
        })
        .await;

        match result {
            Ok(Ok(output)) => {
                let combined = truncate_output(&output.output);
                Ok((combined, output.exit_code))
            }
            Ok(Err(e)) => Err(ToolError::ExecutionFailed(format!("Sandbox error: {}", e))),
            Err(_) => Err(ToolError::Timeout(timeout)),
        }
    }

    /// Execute a command directly (fallback when sandbox unavailable).
    async fn execute_direct(
        &self,
        cmd: &str,
        workdir: &PathBuf,
        timeout: Duration,
        extra_env: &HashMap<String, String>,
    ) -> Result<(String, i32), ToolError> {
        // Build command
        let mut command = if cfg!(target_os = "windows") {
            let mut c = Command::new("cmd");
            c.args(["/C", cmd]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", cmd]);
            c
        };

        // Scrub environment to prevent secret leakage (CWE-200).
        // Only forward known-safe variables; everything else (API keys,
        // session tokens, credentials) is stripped from child processes.
        command.env_clear();
        for var in SAFE_ENV_VARS {
            if let Ok(val) = std::env::var(var) {
                command.env(var, val);
            }
        }

        // Inject extra environment variables (e.g., credentials fetched by the
        // worker runtime) on top of the scrubbed base. These are explicitly
        // provided by the orchestrator and are safe to forward.
        command.envs(extra_env);

        command
            .current_dir(workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Spawn process
        let mut child = command
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to spawn command: {}", e)))?;

        // Drain stdout/stderr concurrently with wait() to prevent deadlocks.
        // If we call wait() without draining the pipes and the child's output
        // exceeds the OS pipe buffer (64KB Linux, 16KB macOS), the child blocks
        // on write and wait() never returns.
        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        let result = tokio::time::timeout(timeout, async {
            let stdout_fut = async {
                if let Some(mut out) = stdout_handle {
                    let mut buf = Vec::new();
                    (&mut out)
                        .take(MAX_OUTPUT_SIZE as u64)
                        .read_to_end(&mut buf)
                        .await
                        .ok();
                    // Drain any remaining output so the child does not block
                    tokio::io::copy(&mut out, &mut tokio::io::sink()).await.ok();
                    String::from_utf8_lossy(&buf).to_string()
                } else {
                    String::new()
                }
            };

            let stderr_fut = async {
                if let Some(mut err) = stderr_handle {
                    let mut buf = Vec::new();
                    (&mut err)
                        .take(MAX_OUTPUT_SIZE as u64)
                        .read_to_end(&mut buf)
                        .await
                        .ok();
                    tokio::io::copy(&mut err, &mut tokio::io::sink()).await.ok();
                    String::from_utf8_lossy(&buf).to_string()
                } else {
                    String::new()
                }
            };

            let (stdout, stderr, wait_result) = tokio::join!(stdout_fut, stderr_fut, child.wait());
            let status = wait_result?;

            // Combine output
            let output = if stderr.is_empty() {
                stdout
            } else if stdout.is_empty() {
                stderr
            } else {
                format!("{}\n\n--- stderr ---\n{}", stdout, stderr)
            };

            Ok::<_, std::io::Error>((output, status.code().unwrap_or(-1)))
        })
        .await;

        match result {
            Ok(Ok((output, code))) => Ok((truncate_output(&output), code)),
            Ok(Err(e)) => Err(ToolError::ExecutionFailed(format!(
                "Command execution failed: {}",
                e
            ))),
            Err(_) => {
                // Timeout - try to kill the process
                let _ = child.kill().await;
                Err(ToolError::Timeout(timeout))
            }
        }
    }

    /// Execute a command, using sandbox if available.
    pub(super) async fn execute_command(
        &self,
        cmd: &str,
        workdir: Option<&str>,
        timeout: Option<u64>,
        extra_env: &HashMap<String, String>,
    ) -> Result<(String, i64), ToolError> {
        // Check for blocked commands
        if let Some(reason) = self.is_blocked(cmd) {
            return Err(ToolError::NotAuthorized(format!(
                "{}: {}",
                reason,
                truncate_for_error(cmd)
            )));
        }

        // Check for injection/obfuscation patterns
        if let Some(reason) = detect_command_injection(cmd) {
            return Err(ToolError::NotAuthorized(format!(
                "Command injection detected ({}): {}",
                reason,
                truncate_for_error(cmd)
            )));
        }

        // Determine working directory
        let cwd = match (&self.working_dir, workdir) {
            (Some(base_dir), Some(path)) => {
                crate::tools::builtin::path_utils::validate_path(path, Some(base_dir))?
            }
            (Some(base_dir), None) => base_dir.clone(),
            (None, Some(path)) => PathBuf::from(path),
            (None, None) => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        };

        // Determine timeout
        let timeout_duration = timeout.map(Duration::from_secs).unwrap_or(self.timeout);

        // Use sandbox if configured; fail-closed (never silently fall through
        // to unsandboxed execution when sandbox was intended).
        if let Some(ref sandbox) = self.sandbox
            && sandbox_active(sandbox)
        {
            return self
                .execute_sandboxed(sandbox, cmd, &cwd, timeout_duration)
                .await;
        }

        // Only execute directly when no sandbox was configured at all.
        let (output, code) = self
            .execute_direct(cmd, &cwd, timeout_duration, extra_env)
            .await?;
        Ok((output, code as i64))
    }
}

/// Whether a configured sandbox must handle execution: it has either
/// finished initialization or is enabled by configuration.
fn sandbox_active(sandbox: &SandboxManager) -> bool {
    sandbox.is_initialized() || sandbox.config().enabled
}

/// Truncate output to fit within limits (UTF-8 safe).
fn truncate_output(s: &str) -> String {
    if s.len() <= MAX_OUTPUT_SIZE {
        s.to_string()
    } else {
        let half = MAX_OUTPUT_SIZE / 2;
        let head_end = crate::util::floor_char_boundary(s, half);
        let tail_start = crate::util::floor_char_boundary(s, s.len() - half);
        format!(
            "{}\n\n... [truncated {} bytes] ...\n\n{}",
            &s[..head_end],
            s.len() - MAX_OUTPUT_SIZE,
            &s[tail_start..]
        )
    }
}

/// Truncate command for error messages (char-aware to avoid UTF-8 boundary panics).
fn truncate_for_error(s: &str) -> String {
    if s.chars().count() <= 100 {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(100).collect::<String>())
    }
}
