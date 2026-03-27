//! Worker completion and status reporting logic.
//!
//! This module handles all interactions with the orchestrator for status updates,
//! completion reports, and job events. It encapsulates the reporting protocol
//! and provides a clean interface for the main worker loop.

use std::sync::Arc;

use crate::agent::agentic_loop::{LoopOutcome, truncate_for_preview};
use crate::error::WorkerError;
use crate::worker::api::{
    CompletionReport, JobEventPayload, JobEventType, StatusUpdate, WorkerState,
};
use crate::worker::container::WorkerRuntime;

/// Execution result discriminator used internally by the worker loop.
pub(super) enum WorkerExecutionResult {
    Outcome(LoopOutcome),
    Failed(crate::error::Error),
    TimedOut,
}

impl WorkerRuntime {
    /// Report a pre-loop failure to the orchestrator and return an error.
    ///
    /// This is called when the worker fails during initialization (e.g., fetching
    /// the job description or hydrating credentials) before the main execution
    /// loop starts.
    pub(super) async fn fail_pre_loop<T>(
        &self,
        stage: &str,
        error: WorkerError,
    ) -> Result<T, WorkerError> {
        tracing::error!(
            job_id = %self.config.job_id,
            stage,
            error = %error,
            "Worker failed before the execution loop started"
        );

        if let Err(report_error) = self
            .report_worker_status(
                WorkerState::Failed,
                Some("pre-loop failure".to_string()),
                100,
            )
            .await
        {
            tracing::warn!(
                job_id = %self.config.job_id,
                stage,
                error = %report_error,
                "Failed to emit terminal pre-loop worker status"
            );
        }

        if let Err(report_error) = self
            .report_failure(100, "Worker failed during startup")
            .await
        {
            tracing::warn!(
                job_id = %self.config.job_id,
                stage,
                error = %report_error,
                "Failed to emit terminal pre-loop completion"
            );
        }

        Err(error)
    }

    /// Report worker status to the orchestrator.
    pub(super) async fn report_worker_status(
        &self,
        state: WorkerState,
        message: Option<String>,
        iteration: u32,
    ) -> Result<(), WorkerError> {
        self.client
            .report_status(&StatusUpdate::new(state, message, iteration))
            .await
    }

    /// Report the final completion state to the orchestrator based on execution result.
    pub(super) async fn report_completion(
        &self,
        execution: WorkerExecutionResult,
        iterations: u32,
    ) -> Result<(), WorkerError> {
        match execution {
            WorkerExecutionResult::Outcome(LoopOutcome::Response(output)) => {
                tracing::info!("Worker completed job {} successfully", self.config.job_id);
                self.post_event(
                    JobEventType::Result,
                    serde_json::json!({
                        "success": true,
                        "message": truncate_for_preview(&output, 2000),
                    }),
                );
                self.client
                    .report_complete(&CompletionReport {
                        success: true,
                        message: Some(output),
                        iterations,
                    })
                    .await
            }
            WorkerExecutionResult::Outcome(LoopOutcome::MaxIterations) => {
                let msg = format!("max iterations ({}) exceeded", self.config.max_iterations);
                tracing::warn!("Worker failed for job {}: {}", self.config.job_id, msg);
                self.report_failure(iterations, &format!("Execution failed: {}", msg))
                    .await
            }
            WorkerExecutionResult::Outcome(LoopOutcome::Stopped | LoopOutcome::NeedApproval(_)) => {
                tracing::info!("Worker for job {} stopped", self.config.job_id);
                self.post_event(
                    JobEventType::Result,
                    serde_json::json!({
                        "success": false,
                        "message": "Execution stopped",
                        "iterations": iterations,
                    }),
                );
                self.client
                    .report_complete(&CompletionReport {
                        success: false,
                        message: Some("Execution stopped".to_string()),
                        iterations,
                    })
                    .await
            }
            WorkerExecutionResult::Failed(error) => {
                tracing::error!("Worker failed for job {}: {}", self.config.job_id, error);
                self.report_failure(iterations, "Execution failed").await
            }
            WorkerExecutionResult::TimedOut => {
                tracing::warn!("Worker timed out for job {}", self.config.job_id);
                self.report_failure(iterations, "Execution timed out").await
            }
        }
    }

    /// Report a failure to the orchestrator.
    pub(super) async fn report_failure(
        &self,
        iterations: u32,
        message: &str,
    ) -> Result<(), WorkerError> {
        self.post_event(
            JobEventType::Result,
            serde_json::json!({
                "success": false,
                "message": message,
            }),
        );
        self.client
            .report_complete(&CompletionReport {
                success: false,
                message: Some(message.to_string()),
                iterations,
            })
            .await
    }

    /// Post a job event to the orchestrator (fire-and-forget).
    ///
    /// Spawns a background task with a bounded timeout to ensure slow event
    /// endpoints cannot delay authoritative completion reports.
    pub(super) fn post_event(&self, event_type: JobEventType, data: serde_json::Value) {
        let client = Arc::clone(&self.client);
        let job_id = self.config.job_id;

        tokio::spawn(async move {
            let payload = JobEventPayload { event_type, data };
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                client.post_event(&payload),
            )
            .await;

            match result {
                Ok(()) => {
                    tracing::debug!(job_id = %job_id, ?event_type, "Posted job event");
                }
                Err(_) => {
                    tracing::warn!(
                        job_id = %job_id,
                        ?event_type,
                        "Job event post timed out after 5s"
                    );
                }
            }
        });
    }
}
