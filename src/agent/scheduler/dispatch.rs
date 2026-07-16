//! Job creation and scheduling.
//!
//! Implements the `dispatch_job*` entry points that create, persist, and
//! schedule jobs, plus the worker-spawning `schedule_with_context` core.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use uuid::Uuid;

use super::{ScheduledJob, Scheduler, WorkerMessage};
use crate::context::JobState;
use crate::error::JobError;
use crate::tools::ApprovalContext;
use crate::worker::job::{Worker, WorkerDeps};

/// Descriptor for a new job to dispatch: who it belongs to, how it is
/// presented, and any initial metadata.
pub struct JobRequest<'a> {
    pub user_id: &'a str,
    pub title: &'a str,
    pub description: &'a str,
    pub metadata: Option<serde_json::Value>,
}

impl Scheduler {
    /// Create, persist, and schedule a job in one shot.
    ///
    /// This is the preferred entry point for dispatching new jobs. It:
    /// 1. Creates the job context via `ContextManager`
    /// 2. Optionally applies metadata (e.g. `max_iterations`)
    /// 3. Persists the job to the database (so FK references from
    ///    `job_actions` / `llm_calls` work immediately)
    /// 4. Schedules the job for worker execution
    ///
    /// Returns the new job ID.
    pub async fn dispatch_job(&self, request: JobRequest<'_>) -> Result<Uuid, JobError> {
        self.dispatch_job_inner(request, None).await
    }

    /// Dispatch a job with an explicit approval context for autonomous execution.
    ///
    /// Same as `dispatch_job`, but the worker will use the given `ApprovalContext`
    /// to determine which tools are pre-approved (instead of blocking all non-`Never` tools).
    pub async fn dispatch_job_with_context(
        &self,
        request: JobRequest<'_>,
        approval_context: ApprovalContext,
    ) -> Result<Uuid, JobError> {
        self.dispatch_job_inner(request, Some(approval_context))
            .await
    }

    /// Shared implementation for `dispatch_job` and `dispatch_job_with_context`.
    async fn dispatch_job_inner(
        &self,
        request: JobRequest<'_>,
        approval_context: Option<ApprovalContext>,
    ) -> Result<Uuid, JobError> {
        let JobRequest {
            user_id,
            title,
            description,
            metadata,
        } = request;
        let job_id = self
            .context_manager
            .create_job_for_user(user_id, title, description)
            .await?;

        // Apply metadata and token budget in a single atomic update.
        // This prevents concurrent workers from observing partial state.
        // Cap user-supplied max_tokens at the configured limit (Issue #815).
        let user_max_tokens = metadata
            .as_ref()
            .and_then(|m| m.get("max_tokens"))
            .and_then(|v| v.as_u64());

        let max_tokens = user_max_tokens
            .map(|user_val| {
                if self.config.max_tokens_per_job == 0 {
                    // Config is "unlimited": use the user-supplied value directly.
                    user_val
                } else {
                    std::cmp::min(user_val, self.config.max_tokens_per_job)
                }
            })
            .unwrap_or(self.config.max_tokens_per_job);

        // Apply both metadata and token budget in one closure (Issue #813: atomic update)
        if let Some(meta) = metadata {
            self.context_manager
                .update_context(job_id, |ctx| {
                    ctx.metadata = meta;
                    if max_tokens > 0 {
                        ctx.max_tokens = max_tokens;
                    }
                })
                .await?;
        } else if max_tokens > 0 {
            self.context_manager
                .update_context(job_id, |ctx| {
                    ctx.max_tokens = max_tokens;
                })
                .await?;
        }

        // Persist to DB before scheduling so the worker's FK references are valid
        if let Some(ref store) = self.store {
            let ctx = self.context_manager.get_context(job_id).await?;
            store.save_job(&ctx).await.map_err(|e| JobError::Failed {
                id: job_id,
                reason: format!("failed to persist job: {e}"),
            })?;
        }

        self.schedule_with_context(job_id, approval_context).await?;
        Ok(job_id)
    }

    /// Schedule a job for execution.
    pub async fn schedule(&self, job_id: Uuid) -> Result<(), JobError> {
        self.schedule_with_context(job_id, None).await
    }

    /// Spawn a background task that removes `job_id` from `jobs` once its
    /// worker handle finishes, preventing unbounded map growth.
    pub(super) fn spawn_cleanup_task(&self, job_id: Uuid) {
        let jobs = Arc::clone(&self.jobs);
        tokio::spawn(async move {
            loop {
                let should_remove = {
                    let jobs_read = jobs.read().await;
                    match jobs_read.get(&job_id) {
                        Some(scheduled) => {
                            scheduled.handle.is_finished() && !scheduled.pending_cancel_persist
                        }
                        None => true,
                    }
                };

                if should_remove {
                    jobs.write().await.remove(&job_id);
                    break;
                }

                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }

    /// Schedule a job with an optional approval context.
    async fn schedule_with_context(
        &self,
        job_id: Uuid,
        approval_context: Option<ApprovalContext>,
    ) -> Result<(), JobError> {
        // Hold write lock for the entire check-insert sequence to prevent
        // TOCTOU races where two concurrent calls both pass the checks.
        {
            let mut jobs = self.jobs.write().await;

            if jobs.contains_key(&job_id) {
                return Ok(());
            }

            if jobs.len() >= self.config.max_parallel_jobs {
                return Err(JobError::MaxJobsExceeded {
                    max: self.config.max_parallel_jobs,
                });
            }

            // Transition job to in_progress
            self.context_manager
                .update_context(job_id, |ctx| {
                    ctx.transition_to(
                        JobState::InProgress,
                        Some("Scheduled for execution".to_string()),
                    )
                })
                .await?
                .map_err(|s| JobError::ContextError {
                    id: job_id,
                    reason: s,
                })?;

            // Create worker channel
            let (tx, rx) = mpsc::channel(16);

            // Create worker with shared dependencies
            let deps = WorkerDeps {
                context_manager: self.context_manager.clone(),
                llm: self.llm.clone(),
                safety: self.safety.clone(),
                tools: self.tools.clone(),
                store: self.store.clone(),
                hooks: self.hooks.clone(),
                timeout: self.config.job_timeout,
                use_planning: self.config.use_planning,
                sse_tx: self.sse_tx.clone(),
                approval_context,
                http_interceptor: self.http_interceptor.clone(),
            };
            let worker = Worker::new(job_id, deps);

            // Spawn worker task
            let handle = tokio::spawn(async move {
                if let Err(e) = worker.run(rx).await {
                    tracing::error!("Worker for job {} failed: {}", job_id, e);
                }
            });

            // Start the worker
            if tx.send(WorkerMessage::Start).await.is_err() {
                tracing::error!(job_id = %job_id, "Worker died before receiving Start message");
            }

            // Insert while still holding the write lock
            jobs.insert(
                job_id,
                ScheduledJob {
                    handle,
                    tx,
                    pending_cancel_persist: false,
                },
            );
        }

        self.spawn_cleanup_task(job_id);

        tracing::info!("Scheduled job {} for execution", job_id);
        Ok(())
    }
}
