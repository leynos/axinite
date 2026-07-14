//! Job stopping, cancellation persistence, and shutdown.
//!
//! Implements graceful stop of individual jobs, bulk shutdown via
//! `stop_all`, and cleanup of finished jobs and sub-tasks.

use std::time::Duration;

use tokio::sync::mpsc;
use uuid::Uuid;

use super::{Scheduler, WorkerMessage};
use crate::context::JobState;
use crate::error::JobError;

const STOP_GRACE_PERIOD: Duration = Duration::from_millis(500);

impl Scheduler {
    async fn stop_in_memory(&self, job_id: Uuid, reason: &str) -> Result<(), JobError> {
        let tx = {
            let jobs = self.jobs.read().await;
            match jobs.get(&job_id) {
                Some(scheduled) => scheduled.tx.clone(),
                None => return Err(JobError::NotFound { id: job_id }),
            }
        };

        self.send_stop_signal(job_id, tx).await;

        // Give the worker a bounded window to observe the stop signal and
        // finish in-flight cleanup before we transition its state.
        tokio::time::sleep(STOP_GRACE_PERIOD).await;

        self.transition_to_cancelled(job_id, reason).await?;
        self.pin_cancel_persist_and_abort(job_id).await;
        Ok(())
    }

    async fn send_stop_signal(&self, job_id: Uuid, tx: mpsc::Sender<WorkerMessage>) {
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            tx.send(WorkerMessage::Stop),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::warn!(
                    job_id = %job_id,
                    reason = %error,
                    "Failed to send stop signal to worker"
                );
            }
            Err(_) => {
                tracing::warn!(
                    job_id = %job_id,
                    timeout_seconds = 5_u64,
                    "Timed out sending stop signal to worker"
                );
            }
        }
    }

    async fn transition_to_cancelled(&self, job_id: Uuid, reason: &str) -> Result<(), JobError> {
        self.context_manager
            .update_context(job_id, |ctx| {
                let current_state = ctx.state;
                if current_state == JobState::Cancelled {
                    return Ok(());
                }
                ctx.transition_to(JobState::Cancelled, Some(reason.to_string()))
                    .map_err(|_| current_state)
            })
            .await?
            .map_err(|from_state| JobError::InvalidTransition {
                id: job_id,
                from_state,
                target: JobState::Cancelled,
            })?;

        Ok(())
    }

    async fn persist_cancelled_status(&self, job_id: Uuid, reason: &str) -> Result<(), JobError> {
        if let Some(ref store) = self.store {
            store
                .update_job_status(job_id, JobState::Cancelled, Some(reason))
                .await
                .map_err(|e| JobError::PersistenceError {
                    id: job_id,
                    reason: e.to_string(),
                })?;
        }

        Ok(())
    }

    async fn pin_cancel_persist_and_abort(&self, job_id: Uuid) {
        let mut jobs = self.jobs.write().await;
        if let Some(scheduled) = jobs.get_mut(&job_id) {
            scheduled.pending_cancel_persist = true;
            if !scheduled.handle.is_finished() {
                scheduled.handle.abort();
            }
        }
    }

    fn should_persist_cancelled_after_timeout(state: JobState) -> bool {
        state == JobState::Cancelled || state.can_transition_to(JobState::Cancelled)
    }

    /// Handle the case where a graceful in-memory stop timed out during
    /// `stop_all`.
    ///
    /// Attempts to force the job to `Cancelled` via
    /// `transition_to_cancelled`, then persists and finalises if the
    /// transition succeeds. Logs an appropriate warning for each failure
    /// mode.
    async fn handle_stop_timeout(
        &self,
        job_id: Uuid,
        reason: &str,
        stop_timeout: tokio::time::Duration,
    ) {
        tracing::warn!(
            job_id = %job_id,
            timeout_seconds = stop_timeout.as_secs(),
            "Timed out stopping job during shutdown"
        );
        match self.transition_to_cancelled(job_id, reason).await {
            Ok(()) => {
                self.pin_cancel_persist_and_abort(job_id).await;
                if let Err(error) = self.persist_cancelled_status(job_id, reason).await {
                    tracing::warn!(
                        job_id = %job_id,
                        %error,
                        "Failed to persist cancellation after shutdown timeout"
                    );
                } else {
                    self.finalize_stop(job_id).await;
                }
            }
            Err(JobError::InvalidTransition { from_state, .. })
                if !Self::should_persist_cancelled_after_timeout(from_state) =>
            {
                tracing::warn!(
                    job_id = %job_id,
                    state = %from_state,
                    "Skipping cancellation persistence after shutdown timeout because the job state no longer permits cancellation"
                );
            }
            Err(error) => {
                tracing::warn!(
                    job_id = %job_id,
                    %error,
                    "Failed to cancel job after shutdown timeout"
                );
            }
        }
    }

    async fn finalize_stop(&self, job_id: Uuid) {
        let mut jobs = self.jobs.write().await;
        if let Some(scheduled) = jobs.get(&job_id)
            && !scheduled.handle.is_finished()
        {
            scheduled.handle.abort();
        }
        jobs.remove(&job_id);
        tracing::info!("Stopped job {}", job_id);
    }

    /// Stop a running job.
    pub async fn stop(&self, job_id: Uuid, reason: &str) -> Result<(), JobError> {
        self.stop_in_memory(job_id, reason).await?;
        self.persist_cancelled_status(job_id, reason).await?;
        self.finalize_stop(job_id).await;

        Ok(())
    }

    /// Clean up finished jobs and subtasks.
    pub async fn cleanup_finished(&self) {
        // Clean up jobs
        {
            let mut jobs = self.jobs.write().await;
            let mut finished = Vec::new();

            for (id, scheduled) in jobs.iter() {
                if scheduled.handle.is_finished() && !scheduled.pending_cancel_persist {
                    finished.push(*id);
                }
            }

            for id in finished {
                jobs.remove(&id);
                tracing::debug!("Cleaned up finished job {}", id);
            }
        }

        // Clean up subtasks
        {
            let mut subtasks = self.subtasks.write().await;
            let mut finished = Vec::new();

            for (id, scheduled) in subtasks.iter() {
                if scheduled.handle.is_finished() {
                    finished.push(*id);
                }
            }

            for id in finished {
                subtasks.remove(&id);
                tracing::trace!("Cleaned up finished subtask {}", id);
            }
        }
    }

    /// Stop all jobs.
    pub async fn stop_all(&self) {
        let job_ids: Vec<Uuid> = self.jobs.read().await.keys().cloned().collect();
        let stop_timeout = tokio::time::Duration::from_secs(5);
        let stop_reason = "Stopped by scheduler";
        let stop_futures = job_ids.into_iter().map(|job_id| async move {
            (
                job_id,
                tokio::time::timeout(stop_timeout, self.stop_in_memory(job_id, stop_reason)).await,
            )
        });

        for (job_id, result) in futures::future::join_all(stop_futures).await {
            match result {
                Ok(Ok(())) => {
                    if let Err(error) = self.persist_cancelled_status(job_id, stop_reason).await {
                        tracing::warn!(
                            job_id = %job_id,
                            %error,
                            "Failed to persist cancellation during shutdown"
                        );
                    } else {
                        self.finalize_stop(job_id).await;
                    }
                }
                Ok(Err(error)) => {
                    tracing::warn!(job_id = %job_id, %error, "Failed to stop job during shutdown");
                }
                Err(_) => {
                    self.handle_stop_timeout(job_id, stop_reason, stop_timeout)
                        .await;
                }
            }
        }

        // Abort all subtasks
        let mut subtasks = self.subtasks.write().await;
        for (_, scheduled) in subtasks.drain() {
            scheduled.handle.abort();
        }
    }
}
