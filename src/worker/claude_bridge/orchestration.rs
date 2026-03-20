//! Runtime orchestration for the Claude bridge worker.

use std::collections::HashMap;
use std::time::Duration;
use tokio::time::Instant;

use crate::error::WorkerError;
use crate::worker::api::{CompletionReport, StatusUpdate, WorkerState};

use super::ClaudeBridgeRuntime;
use super::ndjson::truncate;

const PROMPT_POLL_INTERVAL: Duration = Duration::from_secs(2);
const PROMPT_POLL_ERROR_INTERVAL: Duration = Duration::from_secs(5);

pub(super) struct ClaudeSessionResult {
    pub(super) session_id: Option<String>,
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
        _reason: &str,
        future: impl std::future::Future<Output = Result<T, ClaudeSessionFailure>>,
    ) -> Result<T, ClaudeSessionFailure> {
        future.await
    }

    async fn timeout_followup_loop<T>(
        &self,
        _reason: &str,
        future: impl std::future::Future<Output = Result<T, (u32, ClaudeSessionFailure)>>,
    ) -> Result<T, (u32, ClaudeSessionFailure)> {
        future.await
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
            concat!(
                "No Claude Code auth available. Set ANTHROPIC_API_KEY or run ",
                "`claude login` on the host to authenticate."
            )
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
        let deadline = Instant::now() + self.config.timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err((
                    iteration,
                    ClaudeSessionFailure {
                        error: WorkerError::ExecutionFailed {
                            reason: "Claude follow-up loop timed out".to_string(),
                        },
                        emitted_terminal_result: false,
                    },
                ));
            }

            let prompt = match tokio::time::timeout(remaining, self.poll_for_prompt()).await {
                Err(_) => {
                    return Err((
                        iteration,
                        ClaudeSessionFailure {
                            error: WorkerError::ExecutionFailed {
                                reason: "Claude follow-up loop timed out".to_string(),
                            },
                            emitted_terminal_result: false,
                        },
                    ));
                }
                Ok(result) => match result {
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
                },
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
}
