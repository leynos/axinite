//! Reporting helpers for the Claude bridge runtime.

use crate::error::WorkerError;
use crate::worker::api::{CompletionReport, JobEventPayload, JobEventType, PromptResponse};

use super::ClaudeBridgeRuntime;
use super::orchestration::ClaudeSessionFailure;

impl ClaudeBridgeRuntime {
    pub(super) async fn finish_failure(
        &self,
        log_message: &str,
        iterations: u32,
        failure: &ClaudeSessionFailure,
    ) -> Result<(), WorkerError> {
        tracing::error!(
            job_id = %self.config.job_id,
            "{log_message}: {}",
            failure.error
        );
        self.report_terminal_failure(iterations, failure).await?;
        Ok(())
    }

    pub(super) async fn report_terminal_failure(
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

    pub(super) async fn report_event(&self, event_type: JobEventType, data: &serde_json::Value) {
        let payload = JobEventPayload {
            event_type,
            data: data.clone(),
        };
        if let Err(e) = self.client.post_event(&payload).await {
            tracing::debug!(error = %e, "Failed to report event");
        }
    }

    pub(super) async fn poll_for_prompt(&self) -> Result<Option<PromptResponse>, WorkerError> {
        self.client.poll_prompt().await
    }
}
