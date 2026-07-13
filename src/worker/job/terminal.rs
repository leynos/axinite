//! Terminal job-state transitions with durable persistence and rollback.
//!
//! `mark_completed`, `mark_failed`, and `mark_stuck` move the in-memory
//! [`crate::context::JobContext`] to a terminal state, persist the terminal
//! result atomically, and roll the context back on persistence failure.

use crate::context::JobState;
use crate::error::Error;

use super::Worker;

impl Worker {
    async fn transition_terminal_state<F>(&self, transition: F) -> Result<JobState, Error>
    where
        F: FnOnce(&mut crate::context::JobContext) -> Result<(), String>,
    {
        let previous = self
            .context_manager()
            .update_context(self.job_id, |ctx| {
                let previous = ctx.state;
                let result = if matches!(
                    previous,
                    JobState::Completed | JobState::Failed | JobState::Stuck | JobState::Cancelled
                ) {
                    Err(format!(
                        "Cannot transition from terminal worker state {}",
                        previous
                    ))
                } else {
                    transition(ctx)
                };
                (previous, result)
            })
            .await?;

        let (previous_state, transition_result) = previous;
        transition_result.map_err(|reason| crate::error::JobError::ContextError {
            id: self.job_id,
            reason,
        })?;

        Ok(previous_state)
    }

    /// Mark the job completed and durably persist that terminal outcome.
    ///
    /// Internal scheduler paths and worker unit tests call this once the job's
    /// successful result is known. The method first moves the in-memory
    /// [`JobContext`] to `Completed`, then attempts an atomic terminal
    /// persistence write for the result event and job status. If persistence
    /// fails, it performs a best-effort rollback to the previous in-memory
    /// state before returning the error; callers do not need to issue an extra
    /// rollback step, but they should treat the terminal outcome as not
    /// durable.
    ///
    /// [`JobContext`]: crate::context::JobContext
    pub(crate) async fn mark_completed(&self) -> Result<(), Error> {
        self.apply_terminal_transition(
            JobState::Completed,
            Some("Job completed successfully"),
            "completed",
            "Job completed successfully".to_string(),
            |ctx| {
                ctx.transition_to(
                    JobState::Completed,
                    Some("Job completed successfully".to_string()),
                )
            },
            "mark_completed",
        )
        .await
    }

    /// Roll back the context to the previous state on persistence failure.
    async fn rollback_context(&self, previous: Option<JobState>, operation: &str) {
        if let Some(state) = previous {
            match self
                .context_manager()
                .update_context(self.job_id, |ctx| {
                    ctx.set_state_rollback(state);
                })
                .await
            {
                Ok(()) => {
                    tracing::error!(
                        job_id = %self.job_id,
                        operation,
                        "Rolled back context state after persistence failure"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        job_id = %self.job_id,
                        operation,
                        error = %e,
                        "Failed to roll back context state after persistence failure"
                    );
                }
            }
        }
    }

    async fn apply_terminal_transition<F>(
        &self,
        status: JobState,
        reason: Option<&str>,
        status_str: &str,
        message: String,
        transition: F,
        op_name: &'static str,
    ) -> Result<(), Error>
    where
        F: FnOnce(&mut crate::context::JobContext) -> Result<(), String>,
    {
        let previous = self.transition_terminal_state(transition).await?;
        let event = serde_json::json!({
            "status": status_str,
            "success": matches!(status, JobState::Completed),
            "message": message,
        });
        if let Err(e) = self
            .persist_terminal_result_and_status(status, reason, "result", &event)
            .await
        {
            self.rollback_context(Some(previous), op_name).await;
            return Err(e);
        }
        Ok(())
    }

    /// Mark the job failed and durably persist the terminal failure.
    ///
    /// Internal scheduler paths and unit tests call this when execution has
    /// reached a terminal error. The method updates the in-memory
    /// [`JobContext`] to `Failed`, then attempts one atomic persistence write
    /// for the terminal event and status. If that write fails, it best-effort
    /// rolls the context back to the previous state before returning the
    /// persistence error; callers should not perform additional rollback, but
    /// must treat the failure as non-durable.
    ///
    /// [`JobContext`]: crate::context::JobContext
    pub(crate) async fn mark_failed(&self, reason: &str) -> Result<(), Error> {
        self.apply_terminal_transition(
            JobState::Failed,
            Some(reason),
            "failed",
            format!("Execution failed: {}", reason),
            |ctx| ctx.transition_to(JobState::Failed, Some(reason.to_string())),
            "mark_failed",
        )
        .await
    }

    /// Mark the job stuck and durably persist the terminal stuck result.
    ///
    /// Internal scheduler timeout handling and unit tests call this when the
    /// worker cannot make further progress. The method transitions the
    /// in-memory [`JobContext`] to `Stuck`, then attempts one atomic terminal
    /// persistence write for the result event and status. If persistence
    /// fails, it best-effort rolls the context back to the prior state before
    /// returning the error; callers do not need to clean up the context
    /// themselves, but the stuck outcome should be treated as non-durable.
    ///
    /// [`JobContext`]: crate::context::JobContext
    pub(crate) async fn mark_stuck(&self, reason: &str) -> Result<(), Error> {
        self.apply_terminal_transition(
            JobState::Stuck,
            Some(reason),
            "stuck",
            format!("Job stuck: {}", reason),
            |ctx| ctx.mark_stuck(reason),
            "mark_stuck",
        )
        .await
    }
}
