//! Claude session spawning, stream forwarding, and teardown helpers.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdout, Command};
use tokio::task::JoinHandle;

use crate::error::WorkerError;
use crate::worker::api::{JobEventPayload, JobEventType};

use super::ClaudeBridgeRuntime;
use super::ndjson::{ClaudeStreamEvent, stream_event_to_payloads};
use super::orchestration::{ClaudeSessionFailure, ClaudeSessionResult};

impl ClaudeBridgeRuntime {
    /// Spawn a `claude` CLI process and stream its output.
    pub(super) async fn run_claude_session(
        &self,
        prompt: &str,
        resume_session_id: Option<&str>,
        extra_env: &HashMap<String, String>,
    ) -> Result<ClaudeSessionResult, ClaudeSessionFailure> {
        let mut child = self
            .spawn_claude_child(prompt, resume_session_id, extra_env)
            .await?;

        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                return Err(self
                    .cleanup_failed_session_process(
                        &mut child,
                        None,
                        ClaudeSessionFailure {
                            error: WorkerError::ExecutionFailed {
                                reason: "failed to capture claude stdout".to_string(),
                            },
                            emitted_terminal_result: false,
                        },
                    )
                    .await);
            }
        };

        let stderr = match child.stderr.take() {
            Some(stderr) => stderr,
            None => {
                return Err(self
                    .cleanup_failed_session_process(
                        &mut child,
                        None,
                        ClaudeSessionFailure {
                            error: WorkerError::ExecutionFailed {
                                reason: "failed to capture claude stderr".to_string(),
                            },
                            emitted_terminal_result: false,
                        },
                    )
                    .await);
            }
        };

        let stderr_handle = self.forward_stderr(stderr);
        let (session_id, seen_terminal_result) =
            match tokio::time::timeout(self.config.timeout, self.process_stdout_stream(stdout))
                .await
            {
                Ok(Ok(result)) => result,
                Ok(Err(failure)) => {
                    return Err(self
                        .cleanup_failed_session_process(&mut child, Some(stderr_handle), failure)
                        .await);
                }
                Err(_) => {
                    return Err(self
                        .cleanup_failed_session_process(
                            &mut child,
                            Some(stderr_handle),
                            ClaudeSessionFailure {
                                error: WorkerError::ExecutionFailed {
                                    reason: "Claude session timed out".to_string(),
                                },
                                emitted_terminal_result: false,
                            },
                        )
                        .await);
                }
            };

        self.finalise_session_result(&mut child, stderr_handle, session_id, seen_terminal_result)
            .await
    }

    async fn spawn_claude_child(
        &self,
        prompt: &str,
        resume_session_id: Option<&str>,
        extra_env: &HashMap<String, String>,
    ) -> Result<Child, ClaudeSessionFailure> {
        let mut command = Command::new("claude");
        command
            .arg("-p")
            .arg(prompt)
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose")
            .arg("--max-turns")
            .arg(self.config.max_turns.to_string())
            .arg("--model")
            .arg(&self.config.model)
            .kill_on_drop(true);

        if let Some(session_id) = resume_session_id {
            command.arg("--resume").arg(session_id);
        }

        command.envs(extra_env);
        command
            .current_dir("/workspace")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        command.spawn().map_err(|error| ClaudeSessionFailure {
            error: WorkerError::ExecutionFailed {
                reason: format!("failed to spawn claude: {error}"),
            },
            emitted_terminal_result: false,
        })
    }

    fn forward_stderr(&self, stderr: ChildStderr) -> JoinHandle<()> {
        let client_for_stderr = Arc::clone(&self.client);
        let job_id = self.config.job_id;
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        tracing::debug!(job_id = %job_id, "claude stderr: {line}");
                        let payload = JobEventPayload {
                            event_type: JobEventType::Status,
                            data: serde_json::json!({ "message": line }),
                        };
                        client_for_stderr.post_event(&payload).await;
                    }
                    Ok(None) => break,
                    Err(error) => {
                        tracing::error!(
                            job_id = %job_id,
                            "failed reading claude stderr: {error}"
                        );
                        break;
                    }
                }
            }
        })
    }

    async fn process_stdout_stream(
        &self,
        stdout: ChildStdout,
    ) -> Result<(Option<String>, bool), ClaudeSessionFailure> {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let mut session_id = None;
        let mut seen_terminal_result = false;

        while let Some(raw_line) = lines.next_line().await.map_err(|error| {
            tracing::error!(
                job_id = %self.config.job_id,
                session_id = ?session_id,
                "failed reading claude stdout: {error}"
            );
            ClaudeSessionFailure {
                error: WorkerError::ExecutionFailed {
                    reason: format!("failed reading claude stdout: {error}"),
                },
                emitted_terminal_result: seen_terminal_result,
            }
        })? {
            let line = raw_line.trim().to_string();
            if line.is_empty() {
                continue;
            }

            let parsed_event = serde_json::from_str::<ClaudeStreamEvent>(&line);
            let Ok(event) = parsed_event else {
                let Err(error) = parsed_event else {
                    unreachable!("parsed_event should be an error here");
                };
                tracing::debug!(
                    job_id = %self.config.job_id,
                    "Non-JSON claude output: {} (parse error: {})",
                    line,
                    error
                );
                self.report_event(
                    JobEventType::Status,
                    &serde_json::json!({ "message": line }),
                )
                .await;
                continue;
            };

            if let Some(captured_session_id) = event
                .session_id
                .as_ref()
                .filter(|_| event.event_type == "system")
            {
                session_id = Some(captured_session_id.clone());
                tracing::info!(
                    job_id = %self.config.job_id,
                    session_id = %captured_session_id,
                    "Captured Claude session ID"
                );
            }

            for payload in stream_event_to_payloads(&event) {
                seen_terminal_result |= payload.event_type == JobEventType::Result;
                self.report_event(payload.event_type, &payload.data).await;
            }
        }

        Ok((session_id, seen_terminal_result))
    }

    async fn finalise_session_result(
        &self,
        child: &mut Child,
        stderr_handle: JoinHandle<()>,
        session_id: Option<String>,
        seen_terminal_result: bool,
    ) -> Result<ClaudeSessionResult, ClaudeSessionFailure> {
        let status = match child.wait().await {
            Ok(status) => status,
            Err(error) => {
                return Err(self
                    .cleanup_failed_session_process(
                        child,
                        Some(stderr_handle),
                        ClaudeSessionFailure {
                            error: WorkerError::ExecutionFailed {
                                reason: format!("failed waiting for claude: {error}"),
                            },
                            emitted_terminal_result: seen_terminal_result,
                        },
                    )
                    .await);
            }
        };

        if let Err(error) = stderr_handle.await {
            tracing::debug!(
                job_id = %self.config.job_id,
                "Claude stderr task failed: {error}"
            );
        }

        if !status.success() {
            let code = status.code().unwrap_or(-1);
            tracing::warn!(
                job_id = %self.config.job_id,
                exit_code = code,
                "Claude process exited with non-zero status"
            );

            if !seen_terminal_result {
                self.report_event(
                    JobEventType::Result,
                    &serde_json::json!({
                        "status": "error",
                        "exit_code": code,
                        "session_id": session_id,
                    }),
                )
                .await;
            }

            return Err(ClaudeSessionFailure {
                error: WorkerError::ExecutionFailed {
                    reason: format!("claude exited with code {code}"),
                },
                emitted_terminal_result: true,
            });
        }

        if !seen_terminal_result {
            self.report_event(
                JobEventType::Result,
                &serde_json::json!({
                    "status": "completed",
                    "session_id": session_id,
                }),
            )
            .await;
        }

        Ok(ClaudeSessionResult { session_id })
    }

    pub(super) async fn cleanup_failed_session_process(
        &self,
        child: &mut Child,
        stderr_handle: Option<JoinHandle<()>>,
        failure: ClaudeSessionFailure,
    ) -> ClaudeSessionFailure {
        if let Some(stderr_handle) = stderr_handle {
            stderr_handle.abort();
            if let Err(error) = stderr_handle.await
                && !error.is_cancelled()
            {
                tracing::debug!(
                    job_id = %self.config.job_id,
                    "Claude stderr task failed during cleanup: {error}"
                );
            }
        }

        if let Err(error) = child.kill().await {
            tracing::debug!(
                job_id = %self.config.job_id,
                "failed to kill Claude process during cleanup: {error}"
            );
        }
        if let Err(error) = child.wait().await {
            tracing::debug!(
                job_id = %self.config.job_id,
                "failed to reap Claude process during cleanup: {error}"
            );
        }

        failure
    }
}
