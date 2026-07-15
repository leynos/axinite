//! Job lifecycle commands for the agent.
//!
//! Handles job creation, status checks, cancellation, listing, and stuck-job
//! recovery triggered from user messages or slash commands.

use uuid::Uuid;

use crate::agent::submission::SubmissionResult;
use crate::agent::{Agent, MessageIntent};
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::context::JobContext;
use crate::db::Database;
use crate::error::Error;
use crate::history::{AgentJobSummary, SandboxJobSummary};

/// Parses a job identifier, mapping malformed input to a not-found error.
fn parse_job_id(job_id: &str) -> Result<Uuid, Error> {
    Uuid::parse_str(job_id).map_err(|_| crate::error::JobError::NotFound { id: Uuid::nil() }.into())
}

/// Renders the detailed single-job status shown to the user.
fn format_job_details(ctx: &JobContext) -> String {
    format!(
        "Job: {}\nStatus: {:?}\nCreated: {}\nStarted: {}\nActual cost: {}",
        ctx.title,
        ctx.state,
        ctx.created_at.format("%Y-%m-%d %H:%M:%S"),
        ctx.started_at
            .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "Not started".to_string()),
        ctx.actual_cost
    )
}

/// Wraps a command outcome as a submission result, prefixing any error.
fn to_submission(result: Result<String, Error>, error_prefix: &str) -> SubmissionResult {
    match result {
        Ok(text) => SubmissionResult::response(text),
        Err(e) => SubmissionResult::error(format!("{error_prefix}: {e}")),
    }
}

/// User-supplied parameters for creating a new job.
struct NewJobRequest {
    title: String,
    description: String,
    category: Option<String>,
}

/// Renders formatted job lines under the "Jobs:" heading.
fn render_job_list(lines: Vec<String>) -> String {
    let mut output = String::from("Jobs:\n");
    for line in lines {
        output.push_str(&line);
    }
    output
}

/// Aggregated job counts rendered in the "Jobs summary" line.
#[derive(Default)]
struct JobTotals {
    total: usize,
    in_progress: usize,
    completed: usize,
    failed: usize,
    stuck: usize,
}

impl JobTotals {
    /// Folds an agent-job summary into the totals.
    fn add_agent(&mut self, s: &AgentJobSummary) {
        self.total += s.total;
        self.in_progress += s.in_progress;
        self.completed += s.completed;
        self.failed += s.failed;
        self.stuck += s.stuck;
    }

    /// Folds a sandbox-job summary into the totals; interrupted jobs count as
    /// failed and running jobs count as in progress.
    fn add_sandbox(&mut self, s: &SandboxJobSummary) {
        self.total += s.total;
        self.in_progress += s.running;
        self.completed += s.completed;
        self.failed += s.failed + s.interrupted;
    }

    /// Renders the one-line summary shown to the user.
    fn render(&self) -> String {
        format!(
            "Jobs summary: Total: {} In Progress: {} Completed: {} Failed: {} Stuck: {}",
            self.total, self.in_progress, self.completed, self.failed, self.stuck
        )
    }
}

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
                self.handle_create_job(
                    &message.user_id,
                    NewJobRequest {
                        title,
                        description,
                        category,
                    },
                )
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
        request: NewJobRequest,
    ) -> Result<String, Error> {
        let NewJobRequest {
            title,
            description,
            category,
        } = request;
        let job_id = self
            .scheduler
            .dispatch_job(crate::agent::scheduler::JobRequest {
                user_id,
                title: &title,
                description: &description,
                metadata: None,
            })
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

    /// Fetches a job context, treating jobs owned by other users as missing.
    async fn owned_context(&self, user_id: &str, uuid: Uuid) -> Result<JobContext, Error> {
        let ctx = self.context_manager.get_context(uuid).await?;
        if ctx.user_id != user_id {
            return Err(crate::error::JobError::NotFound { id: uuid }.into());
        }
        Ok(ctx)
    }

    /// Renders the detailed status of a single job, preferring persistent
    /// state from the store over the in-memory context manager.
    async fn job_status_details(&self, user_id: &str, id: &str) -> Result<String, Error> {
        let uuid = parse_job_id(id)?;
        if let Some(store) = self.store()
            && let Ok(Some(ctx)) = store.get_job(uuid).await
        {
            return Ok(format_job_details(&ctx));
        }
        let ctx = self.owned_context(user_id, uuid).await?;
        Ok(format_job_details(&ctx))
    }

    /// Renders the aggregate jobs summary, preferring the store for
    /// consistency with the Jobs tab.
    async fn jobs_summary(&self, user_id: &str) -> String {
        if let Some(store) = self.store() {
            let mut totals = JobTotals::default();
            if let Ok(s) = store.agent_job_summary().await {
                totals.add_agent(&s);
            }
            if let Ok(s) = store.sandbox_job_summary().await {
                totals.add_sandbox(&s);
            }
            return totals.render();
        }

        // Fallback to ContextManager if no DB.
        let summary = self.context_manager.summary_for(user_id).await;
        JobTotals {
            total: summary.total,
            in_progress: summary.in_progress,
            completed: summary.completed,
            failed: summary.failed,
            stuck: summary.stuck,
        }
        .render()
    }

    async fn handle_check_status(
        &self,
        user_id: &str,
        job_id: Option<String>,
    ) -> Result<String, Error> {
        match job_id {
            Some(id) => self.job_status_details(user_id, &id).await,
            None => Ok(self.jobs_summary(user_id).await),
        }
    }

    async fn handle_cancel_job(&self, user_id: &str, job_id: &str) -> Result<String, Error> {
        let uuid = parse_job_id(job_id)?;
        self.owned_context(user_id, uuid).await?;
        self.scheduler.stop(uuid, "Cancelled by user").await?;

        Ok(format!("Job {} has been cancelled.", job_id))
    }

    /// Lists jobs from the persistent store, warning (rather than failing)
    /// when either job table cannot be read.
    async fn list_jobs_from_store(store: &std::sync::Arc<dyn Database>) -> String {
        let agent_jobs = store.list_agent_jobs().await.unwrap_or_else(|e| {
            tracing::warn!("Failed to list agent jobs: {}", e);
            Vec::new()
        });
        let sandbox_jobs = store.list_sandbox_jobs().await.unwrap_or_else(|e| {
            tracing::warn!("Failed to list sandbox jobs: {}", e);
            Vec::new()
        });

        if agent_jobs.is_empty() && sandbox_jobs.is_empty() {
            return "No jobs found.".to_string();
        }

        let mut lines: Vec<String> = agent_jobs
            .iter()
            .map(|j| format!("  {} - {} ({})\n", j.id, j.title, j.status))
            .collect();
        lines.extend(
            sandbox_jobs
                .iter()
                .map(|j| format!("  {} - {} ({})\n", j.id, j.task, j.status)),
        );
        render_job_list(lines)
    }

    /// Lists jobs from the in-memory context manager (no-DB fallback).
    async fn list_jobs_from_contexts(&self, user_id: &str) -> String {
        let jobs = self.context_manager.all_jobs_for(user_id).await;
        if jobs.is_empty() {
            return "No jobs found.".to_string();
        }

        let mut lines = Vec::new();
        for job_id in jobs {
            if let Ok(ctx) = self.context_manager.get_context(job_id).await {
                lines.push(format!("  {} - {} ({:?})\n", job_id, ctx.title, ctx.state));
            }
        }
        render_job_list(lines)
    }

    async fn handle_list_jobs(
        &self,
        user_id: &str,
        _filter: Option<String>,
    ) -> Result<String, Error> {
        // List from DB for consistency with Jobs tab.
        if let Some(store) = self.store() {
            return Ok(Self::list_jobs_from_store(store).await);
        }

        // Fallback to ContextManager if no DB.
        Ok(self.list_jobs_from_contexts(user_id).await)
    }

    async fn handle_help_job(&self, user_id: &str, job_id: &str) -> Result<String, Error> {
        let uuid = parse_job_id(job_id)?;
        let ctx = self.owned_context(user_id, uuid).await?;

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
        let result = self
            .handle_check_status(user_id, job_id.map(|s| s.to_string()))
            .await;
        Ok(to_submission(result, "Job status error"))
    }

    /// Cancel a job by ID.
    pub(in crate::agent) async fn process_job_cancel(
        &self,
        user_id: &str,
        job_id: &str,
    ) -> Result<SubmissionResult, Error> {
        Ok(to_submission(
            self.handle_cancel_job(user_id, job_id).await,
            "Cancel error",
        ))
    }
}
