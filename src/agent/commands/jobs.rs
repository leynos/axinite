//! Job lifecycle commands for the agent.
//!
//! Handles job creation, status checks, cancellation, listing, and stuck-job
//! recovery triggered from user messages or slash commands.

use uuid::Uuid;

use crate::agent::submission::SubmissionResult;
use crate::agent::{Agent, MessageIntent};
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::error::Error;

impl Agent {
    /// Handle job-related intents without turn tracking.
    pub(in crate::agent) async fn handle_job_or_command(
        &self,
        intent: MessageIntent,
        message: &IncomingMessage,
    ) -> Result<SubmissionResult, Error> {
        // Send thinking status for non-trivial operations
        if let MessageIntent::CreateJob { .. } = &intent {
            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::Thinking("Processing...".into()),
                    &message.metadata,
                )
                .await;
        }

        let response = match intent {
            MessageIntent::CreateJob {
                title,
                description,
                category,
            } => {
                self.handle_create_job(&message.user_id, title, description, category)
                    .await?
            }
            MessageIntent::CheckJobStatus { job_id } => {
                self.handle_check_status(&message.user_id, job_id).await?
            }
            MessageIntent::CancelJob { job_id } => {
                self.handle_cancel_job(&message.user_id, &job_id).await?
            }
            MessageIntent::ListJobs { filter } => {
                self.handle_list_jobs(&message.user_id, filter).await?
            }
            MessageIntent::HelpJob { job_id } => {
                self.handle_help_job(&message.user_id, &job_id).await?
            }
            MessageIntent::Command { command, args } => {
                match self
                    .handle_command(&command, &args, &message.channel)
                    .await?
                {
                    Some(s) => s,
                    None => return Ok(SubmissionResult::Ok { message: None }), // Shutdown signal
                }
            }
            _ => "Unknown intent".to_string(),
        };
        Ok(SubmissionResult::response(response))
    }

    async fn handle_create_job(
        &self,
        user_id: &str,
        title: String,
        description: String,
        category: Option<String>,
    ) -> Result<String, Error> {
        let job_id = self
            .scheduler
            .dispatch_job(user_id, &title, &description, None)
            .await?;

        // Set the dedicated category field (not stored in metadata)
        if let Some(cat) = category
            && let Err(e) = self
                .context_manager
                .update_context(job_id, |ctx| {
                    ctx.category = Some(cat);
                })
                .await
        {
            tracing::warn!(job_id = %job_id, "Failed to set job category: {}", e);
        }

        Ok(format!(
            "Created job: {}\nID: {}\n\nThe job has been scheduled and is now running.",
            title, job_id
        ))
    }

    async fn handle_check_status(
        &self,
        user_id: &str,
        job_id: Option<String>,
    ) -> Result<String, Error> {
        match job_id {
            Some(id) => {
                let uuid = Uuid::parse_str(&id)
                    .map_err(|_| crate::error::JobError::NotFound { id: Uuid::nil() })?;

                // Try DB first for persistent state, fall back to ContextManager.
                if let Some(store) = self.store()
                    && let Ok(Some(ctx)) = store.get_job(uuid).await
                {
                    return Ok(format!(
                        "Job: {}\nStatus: {:?}\nCreated: {}\nStarted: {}\nActual cost: {}",
                        ctx.title,
                        ctx.state,
                        ctx.created_at.format("%Y-%m-%d %H:%M:%S"),
                        ctx.started_at
                            .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
                            .unwrap_or_else(|| "Not started".to_string()),
                        ctx.actual_cost
                    ));
                }

                let ctx = self.context_manager.get_context(uuid).await?;
                if ctx.user_id != user_id {
                    return Err(crate::error::JobError::NotFound { id: uuid }.into());
                }

                Ok(format!(
                    "Job: {}\nStatus: {:?}\nCreated: {}\nStarted: {}\nActual cost: {}",
                    ctx.title,
                    ctx.state,
                    ctx.created_at.format("%Y-%m-%d %H:%M:%S"),
                    ctx.started_at
                        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| "Not started".to_string()),
                    ctx.actual_cost
                ))
            }
            None => {
                // Show summary from DB for consistency with Jobs tab.
                if let Some(store) = self.store() {
                    let mut total = 0;
                    let mut in_progress = 0;
                    let mut completed = 0;
                    let mut failed = 0;
                    let mut stuck = 0;

                    if let Ok(s) = store.agent_job_summary().await {
                        total += s.total;
                        in_progress += s.in_progress;
                        completed += s.completed;
                        failed += s.failed;
                        stuck += s.stuck;
                    }
                    if let Ok(s) = store.sandbox_job_summary().await {
                        total += s.total;
                        in_progress += s.running;
                        completed += s.completed;
                        failed += s.failed + s.interrupted;
                    }

                    return Ok(format!(
                        "Jobs summary: Total: {} In Progress: {} Completed: {} Failed: {} Stuck: {}",
                        total, in_progress, completed, failed, stuck
                    ));
                }

                // Fallback to ContextManager if no DB.
                let summary = self.context_manager.summary_for(user_id).await;
                Ok(format!(
                    "Jobs summary: Total: {} In Progress: {} Completed: {} Failed: {} Stuck: {}",
                    summary.total,
                    summary.in_progress,
                    summary.completed,
                    summary.failed,
                    summary.stuck
                ))
            }
        }
    }

    async fn handle_cancel_job(&self, user_id: &str, job_id: &str) -> Result<String, Error> {
        let uuid = Uuid::parse_str(job_id)
            .map_err(|_| crate::error::JobError::NotFound { id: Uuid::nil() })?;

        let ctx = self.context_manager.get_context(uuid).await?;
        if ctx.user_id != user_id {
            return Err(crate::error::JobError::NotFound { id: uuid }.into());
        }

        self.scheduler.stop(uuid, "Cancelled by user").await?;

        Ok(format!("Job {} has been cancelled.", job_id))
    }

    async fn handle_list_jobs(
        &self,
        user_id: &str,
        _filter: Option<String>,
    ) -> Result<String, Error> {
        // List from DB for consistency with Jobs tab.
        if let Some(store) = self.store() {
            let agent_jobs = match store.list_agent_jobs().await {
                Ok(jobs) => jobs,
                Err(e) => {
                    tracing::warn!("Failed to list agent jobs: {}", e);
                    Vec::new()
                }
            };
            let sandbox_jobs = match store.list_sandbox_jobs().await {
                Ok(jobs) => jobs,
                Err(e) => {
                    tracing::warn!("Failed to list sandbox jobs: {}", e);
                    Vec::new()
                }
            };

            if agent_jobs.is_empty() && sandbox_jobs.is_empty() {
                return Ok("No jobs found.".to_string());
            }

            let mut output = String::from("Jobs:\n");
            for j in &agent_jobs {
                output.push_str(&format!("  {} - {} ({})\n", j.id, j.title, j.status));
            }
            for j in &sandbox_jobs {
                output.push_str(&format!("  {} - {} ({})\n", j.id, j.task, j.status));
            }
            return Ok(output);
        }

        // Fallback to ContextManager if no DB.
        let jobs = self.context_manager.all_jobs_for(user_id).await;
        if jobs.is_empty() {
            return Ok("No jobs found.".to_string());
        }

        let mut output = String::from("Jobs:\n");
        for job_id in jobs {
            if let Ok(ctx) = self.context_manager.get_context(job_id).await {
                output.push_str(&format!("  {} - {} ({:?})\n", job_id, ctx.title, ctx.state));
            }
        }
        Ok(output)
    }

    async fn handle_help_job(&self, user_id: &str, job_id: &str) -> Result<String, Error> {
        let uuid = Uuid::parse_str(job_id)
            .map_err(|_| crate::error::JobError::NotFound { id: Uuid::nil() })?;

        let ctx = self.context_manager.get_context(uuid).await?;
        if ctx.user_id != user_id {
            return Err(crate::error::JobError::NotFound { id: uuid }.into());
        }

        if ctx.state == crate::context::JobState::Stuck {
            // Attempt recovery
            self.context_manager
                .update_context(uuid, |ctx| ctx.attempt_recovery())
                .await?
                .map_err(|e| crate::error::JobError::ContextError {
                    id: uuid,
                    reason: e.to_string(),
                })?;

            // Reschedule
            self.scheduler.schedule(uuid).await?;

            Ok(format!(
                "Job {} was stuck. Attempting recovery (attempt #{}).",
                job_id,
                ctx.repair_attempts + 1
            ))
        } else {
            Ok(format!(
                "Job {} is not stuck (current state: {:?}). No help needed.",
                job_id, ctx.state
            ))
        }
    }

    /// Show job status inline — either all jobs (no id) or a specific job.
    pub(in crate::agent) async fn process_job_status(
        &self,
        user_id: &str,
        job_id: Option<&str>,
    ) -> Result<SubmissionResult, Error> {
        match self
            .handle_check_status(user_id, job_id.map(|s| s.to_string()))
            .await
        {
            Ok(text) => Ok(SubmissionResult::response(text)),
            Err(e) => Ok(SubmissionResult::error(format!("Job status error: {}", e))),
        }
    }

    /// Cancel a job by ID.
    pub(in crate::agent) async fn process_job_cancel(
        &self,
        user_id: &str,
        job_id: &str,
    ) -> Result<SubmissionResult, Error> {
        match self.handle_cancel_job(user_id, job_id).await {
            Ok(text) => Ok(SubmissionResult::response(text)),
            Err(e) => Ok(SubmissionResult::error(format!("Cancel error: {}", e))),
        }
    }
}
