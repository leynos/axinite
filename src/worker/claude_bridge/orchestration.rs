//! Runtime orchestration for the Claude bridge worker.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::error::WorkerError;
use crate::worker::api::{
    CompletionReport, JobEventPayload, JobEventType, PromptResponse, StatusUpdate, WorkerState,
};

use super::ClaudeBridgeRuntime;
use super::ndjson::{ClaudeStreamEvent, stream_event_to_payloads, truncate};

const PROMPT_POLL_INTERVAL: Duration = Duration::from_secs(2);
const PROMPT_POLL_ERROR_INTERVAL: Duration = Duration::from_secs(5);

struct ClaudeSessionResult {
    session_id: Option<String>,
}

struct ClaudeSessionFailure {
    error: WorkerError,
    emitted_terminal_result: bool,
}

impl ClaudeBridgeRuntime {
    /// Run the bridge: fetch job, spawn claude, stream events, handle follow-ups.
    pub async fn run(&self) -> Result<(), WorkerError> {
        self.copy_auth_from_mount().await?;
        self.write_permission_settings().await?;

        let job = self.client.get_job().await?;

        tracing::info!(
            job_id = %self.config.job_id,
            "Starting Claude Code bridge for: {}",
            truncate(&job.description, 100)
        );

        let credentials = self.client.fetch_credentials().await?;
        let mut extra_env = HashMap::new();
        for credential in &credentials {
            extra_env.insert(credential.env_var.clone(), credential.value.clone());
        }
        if !extra_env.is_empty() {
            tracing::info!(
                job_id = %self.config.job_id,
                "Fetched {} credential(s) for child process injection",
                extra_env.len()
            );
        }

        let has_api_key = extra_env.contains_key("ANTHROPIC_API_KEY")
            || std::env::var("ANTHROPIC_API_KEY").is_ok();
        let has_oauth = extra_env.contains_key("CLAUDE_CODE_OAUTH_TOKEN")
            || std::env::var("CLAUDE_CODE_OAUTH_TOKEN").is_ok();
        if !has_api_key && !has_oauth {
            tracing::warn!(
                job_id = %self.config.job_id,
                "No Claude Code auth available. Set ANTHROPIC_API_KEY or run \
                 `claude login` on the host to authenticate."
            );
        }

        self.client
            .report_status(&StatusUpdate::new(
                WorkerState::Running,
                Some("Spawning Claude Code".to_string()),
                0,
            ))
            .await?;

        let session = match self
            .run_claude_session(&job.description, None, &extra_env)
            .await
        {
            Ok(session) => session,
            Err(failure) => {
                tracing::error!(
                    job_id = %self.config.job_id,
                    "Claude session failed: {}",
                    failure.error
                );
                self.report_terminal_failure(1, &failure).await?;
                return Ok(());
            }
        };
        let session_id = session.session_id;

        let mut iteration = 1u32;
        loop {
            match self.poll_for_prompt().await {
                Ok(Some(prompt)) => {
                    if prompt.done {
                        tracing::info!(job_id = %self.config.job_id, "Orchestrator signaled done");
                        break;
                    }
                    iteration += 1;
                    tracing::info!(
                        job_id = %self.config.job_id,
                        "Got follow-up prompt, resuming session"
                    );
                    if let Err(failure) = self
                        .run_claude_session(&prompt.content, session_id.as_deref(), &extra_env)
                        .await
                    {
                        tracing::error!(
                            job_id = %self.config.job_id,
                            "Follow-up Claude session failed: {}",
                            failure.error
                        );
                        self.report_terminal_failure(iteration, &failure).await?;
                        return Ok(());
                    }
                }
                Ok(None) => {
                    tokio::time::sleep(PROMPT_POLL_INTERVAL).await;
                }
                Err(error) => {
                    tracing::warn!(
                        job_id = %self.config.job_id,
                        "Prompt polling error: {}", error
                    );
                    tokio::time::sleep(PROMPT_POLL_ERROR_INTERVAL).await;
                }
            }
        }

        self.client
            .report_complete(&CompletionReport {
                success: true,
                message: Some("Claude Code session completed".to_string()),
                iterations: iteration,
            })
            .await?;

        Ok(())
    }

    async fn report_terminal_failure(
        &self,
        iterations: u32,
        failure: &ClaudeSessionFailure,
    ) -> Result<(), WorkerError> {
        if !failure.emitted_terminal_result {
            self.report_event(
                JobEventType::Result,
                &serde_json::json!({
                    "success": false,
                    "message": failure.error.to_string(),
                }),
            )
            .await;
        }
        self.client
            .report_complete(&CompletionReport {
                success: false,
                message: Some("Claude Code failed".to_string()),
                iterations,
            })
            .await
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

        let stdout = child.stdout.take().ok_or_else(|| ClaudeSessionFailure {
            error: WorkerError::ExecutionFailed {
                reason: "failed to capture claude stdout".to_string(),
            },
            emitted_terminal_result: false,
        })?;

        let stderr = child.stderr.take().ok_or_else(|| ClaudeSessionFailure {
            error: WorkerError::ExecutionFailed {
                reason: "failed to capture claude stderr".to_string(),
            },
            emitted_terminal_result: false,
        })?;

        let client_for_stderr = Arc::clone(&self.client);
        let job_id = self.config.job_id;
        let stderr_handle = tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(job_id = %job_id, "claude stderr: {}", line);
                let payload = JobEventPayload {
                    event_type: JobEventType::Status,
                    data: serde_json::json!({ "message": line }),
                };
                client_for_stderr.post_event(&payload).await;
            }
        });

        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let mut session_id: Option<String> = None;
        let mut seen_terminal_result = false;

        while let Ok(Some(line)) = lines.next_line().await {
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
                emitted_terminal_result: seen_terminal_result,
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

    async fn report_event(&self, event_type: JobEventType, data: &serde_json::Value) {
        let payload = JobEventPayload {
            event_type,
            data: data.clone(),
        };
        self.client.post_event(&payload).await;
    }

    async fn poll_for_prompt(&self) -> Result<Option<PromptResponse>, WorkerError> {
        self.client.poll_prompt().await
    }
}
