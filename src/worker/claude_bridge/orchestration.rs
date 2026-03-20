//! Runtime orchestration for the Claude bridge worker.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::error::WorkerError;
use crate::worker::api::{
    CompletionReport, JobEventPayload, JobEventType, StatusUpdate, WorkerState,
};

use super::ClaudeBridgeRuntime;
use super::ndjson::{ClaudeStreamEvent, stream_event_to_payloads, truncate};

const PROMPT_POLL_INTERVAL: Duration = Duration::from_secs(2);
const PROMPT_POLL_ERROR_INTERVAL: Duration = Duration::from_secs(5);

struct ClaudeSessionResult {
    session_id: Option<String>,
}

/// Records whether `emitted_terminal_result` already sent the final result
/// event. `true` avoids duplicate terminal emissions; `false` tells callers
/// they still need to publish the final failure result during cleanup.
pub(super) struct ClaudeSessionFailure {
    pub(super) error: WorkerError,
    pub(super) emitted_terminal_result: bool,
}

impl ClaudeBridgeRuntime {
    /// Run the bridge: fetch job, spawn claude, stream events, handle follow-ups.
    pub async fn run(&self) -> Result<(), WorkerError> {
        let copied_auth_present = self.preflight_fs().await?;
        let (job_description, extra_env) = self.fetch_job_and_env().await?;
        self.warn_if_missing_auth(&extra_env, copied_auth_present);
        self.client
            .report_status(&StatusUpdate::new(
                WorkerState::Running,
                Some("Spawning Claude Code".to_string()),
                0,
            ))
            .await?;

        let session_id = match self
            .timeout_initial_session(
                "initial Claude session timed out",
                self.run_initial_session(&job_description, &extra_env),
            )
            .await
        {
            Ok(session_id) => session_id,
            Err(failure) => {
                return self
                    .finish_failure("Claude session failed", 1, &failure)
                    .await;
            }
        };

        match self
            .timeout_followup_loop(
                "Claude follow-up loop timed out",
                self.followup_loop(session_id, &extra_env),
            )
            .await
        {
            Ok(iterations) => {
                self.client
                    .report_complete(&CompletionReport {
                        success: true,
                        message: Some("Claude Code session completed".to_string()),
                        iterations,
                    })
                    .await
            }
            Err((iterations, failure)) => {
                self.finish_failure("Follow-up Claude session failed", iterations, &failure)
                    .await
            }
        }
    }

    async fn timeout_initial_session<T>(
        &self,
        reason: &str,
        future: impl std::future::Future<Output = Result<T, ClaudeSessionFailure>>,
    ) -> Result<T, ClaudeSessionFailure> {
        match tokio::time::timeout(self.config.timeout, future).await {
            Ok(result) => result,
            Err(_) => Err(ClaudeSessionFailure {
                error: WorkerError::ExecutionFailed {
                    reason: reason.to_string(),
                },
                emitted_terminal_result: false,
            }),
        }
    }

    async fn timeout_followup_loop<T>(
        &self,
        reason: &str,
        future: impl std::future::Future<Output = Result<T, (u32, ClaudeSessionFailure)>>,
    ) -> Result<T, (u32, ClaudeSessionFailure)> {
        match tokio::time::timeout(self.config.timeout, future).await {
            Ok(result) => result,
            Err(_) => Err((
                1,
                ClaudeSessionFailure {
                    error: WorkerError::ExecutionFailed {
                        reason: reason.to_string(),
                    },
                    emitted_terminal_result: false,
                },
            )),
        }
    }

    async fn preflight_fs(&self) -> Result<bool, WorkerError> {
        self.copy_auth_from_mount().await?;
        self.write_permission_settings().await?;
        self.has_copied_auth().await
    }

    async fn fetch_job_and_env(&self) -> Result<(String, HashMap<String, String>), WorkerError> {
        let job = self.client.get_job().await?;
        tracing::info!(
            job_id = %self.config.job_id,
            "Starting Claude Code bridge for: {}",
            truncate(&job.description, 100)
        );

        let credentials = self.client.fetch_credentials().await?;
        let extra_env = self.build_child_env(&credentials);
        if !extra_env.is_empty() {
            tracing::info!(
                job_id = %self.config.job_id,
                "Fetched {} credential(s) for child process injection",
                extra_env.len()
            );
        }

        Ok((job.description, extra_env))
    }

    fn build_child_env(
        &self,
        credentials: &[crate::worker::api::CredentialResponse],
    ) -> HashMap<String, String> {
        credentials
            .iter()
            .map(|credential| (credential.env_var.clone(), credential.value.clone()))
            .collect()
    }

    fn warn_if_missing_auth(&self, env: &HashMap<String, String>, copied_auth_present: bool) {
        let has_api_key =
            env.contains_key("ANTHROPIC_API_KEY") || std::env::var("ANTHROPIC_API_KEY").is_ok();
        let has_oauth = env.contains_key("CLAUDE_CODE_OAUTH_TOKEN")
            || std::env::var("CLAUDE_CODE_OAUTH_TOKEN").is_ok();
        if has_api_key || has_oauth || copied_auth_present {
            return;
        }
        tracing::warn!(
            job_id = %self.config.job_id,
            "No Claude Code auth available. Set ANTHROPIC_API_KEY or run \
             `claude login` on the host to authenticate."
        );
    }

    async fn run_initial_session(
        &self,
        prompt: &str,
        env: &HashMap<String, String>,
    ) -> Result<Option<String>, ClaudeSessionFailure> {
        self.run_claude_session(prompt, None, env)
            .await
            .map(|session| session.session_id)
    }

    async fn followup_loop(
        &self,
        mut session_id: Option<String>,
        env: &HashMap<String, String>,
    ) -> Result<u32, (u32, ClaudeSessionFailure)> {
        let mut iteration = 1u32;
        loop {
            let prompt = match self.poll_for_prompt().await {
                Ok(Some(prompt)) => prompt,
                Ok(None) => {
                    tokio::time::sleep(PROMPT_POLL_INTERVAL).await;
                    continue;
                }
                Err(error) => {
                    tracing::warn!(
                        job_id = %self.config.job_id,
                        "Prompt polling error: {}", error
                    );
                    tokio::time::sleep(PROMPT_POLL_ERROR_INTERVAL).await;
                    continue;
                }
            };

            if prompt.done {
                tracing::info!(job_id = %self.config.job_id, "Orchestrator signaled done");
                return Ok(iteration);
            }

            iteration += 1;
            let Some(current_session_id) = session_id.as_deref() else {
                return Err((
                    iteration,
                    ClaudeSessionFailure {
                        error: WorkerError::ExecutionFailed {
                            reason: "missing Claude session id for follow-up resume".to_string(),
                        },
                        emitted_terminal_result: false,
                    },
                ));
            };
            tracing::info!(
                job_id = %self.config.job_id,
                "Got follow-up prompt, resuming session"
            );

            match self
                .run_claude_session(&prompt.content, Some(current_session_id), env)
                .await
            {
                Ok(result) => {
                    if let Some(next_session_id) = result.session_id {
                        session_id = Some(next_session_id);
                    }
                }
                Err(failure) => return Err((iteration, failure)),
            }
        }
    }

    /// Spawn a `claude` CLI process and stream its output.
    async fn run_claude_session(
        &self,
        prompt: &str,
        resume_session_id: Option<&str>,
        extra_env: &HashMap<String, String>,
    ) -> Result<ClaudeSessionResult, ClaudeSessionFailure> {
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
            .arg(&self.config.model);

        if let Some(session_id) = resume_session_id {
            command.arg("--resume").arg(session_id);
        }

        command.envs(extra_env);
        command
            .current_dir("/workspace")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = command.spawn().map_err(|error| ClaudeSessionFailure {
            error: WorkerError::ExecutionFailed {
                reason: format!("failed to spawn claude: {}", error),
            },
            emitted_terminal_result: false,
        })?;

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

        let client_for_stderr = Arc::clone(&self.client);
        let job_id = self.config.job_id;
        let stderr_handle = tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        tracing::debug!(job_id = %job_id, "claude stderr: {}", line);
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
                            "failed reading claude stderr: {}",
                            error
                        );
                        break;
                    }
                }
            }
        });

        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let mut session_id: Option<String> = None;
        let mut seen_terminal_result = false;

        loop {
            let line = match lines.next_line().await {
                Ok(Some(line)) => line,
                Ok(None) => break,
                Err(error) => {
                    return Err(self
                        .cleanup_failed_session_process(
                            &mut child,
                            Some(stderr_handle),
                            ClaudeSessionFailure {
                                error: WorkerError::ExecutionFailed {
                                    reason: format!("failed reading claude stdout: {}", error),
                                },
                                emitted_terminal_result: seen_terminal_result,
                            },
                        )
                        .await);
                }
            };

            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<ClaudeStreamEvent>(&line) {
                Ok(event) => {
                    if event.event_type == "system"
                        && let Some(ref captured_session_id) = event.session_id
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
                Err(error) => {
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
                }
            }
        }

        let status = child.wait().await.map_err(|error| ClaudeSessionFailure {
            error: WorkerError::ExecutionFailed {
                reason: format!("failed waiting for claude: {}", error),
            },
            emitted_terminal_result: seen_terminal_result,
        })?;

        if let Err(error) = stderr_handle.await {
            tracing::debug!(
                job_id = %self.config.job_id,
                "Claude stderr task failed: {}", error
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
                    reason: format!("claude exited with code {}", code),
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

    async fn cleanup_failed_session_process(
        &self,
        child: &mut tokio::process::Child,
        stderr_handle: Option<tokio::task::JoinHandle<()>>,
        failure: ClaudeSessionFailure,
    ) -> ClaudeSessionFailure {
        if let Some(stderr_handle) = stderr_handle {
            stderr_handle.abort();
            if let Err(error) = stderr_handle.await
                && !error.is_cancelled()
            {
                tracing::debug!(
                    job_id = %self.config.job_id,
                    "Claude stderr task failed during cleanup: {}",
                    error
                );
            }
        }

        if let Err(error) = child.kill().await {
            tracing::debug!(
                job_id = %self.config.job_id,
                "failed to kill Claude process during cleanup: {}",
                error
            );
        }
        if let Err(error) = child.wait().await {
            tracing::debug!(
                job_id = %self.config.job_id,
                "failed to reap Claude process during cleanup: {}",
                error
            );
        }

        failure
    }
}
