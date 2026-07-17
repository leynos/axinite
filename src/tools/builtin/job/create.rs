//! The `create_job` tool: struct, builders, local execution, and tool wiring.
//!
//! `CreateJobTool` creates jobs either locally (via the Scheduler or
//! ContextManager) or in a sandboxed Docker container when sandbox deps are
//! injected. The sandbox execution path lives in [`super::sandbox`]; credential
//! parsing lives in [`super::credentials`].

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use uuid::Uuid;

use crate::channels::IncomingMessage;
use crate::channels::web::types::SseEvent;
use crate::context::{ContextManager, JobContext};
use crate::db::{Database, SandboxJobStatusUpdate};
use crate::orchestrator::job_manager::{ContainerJobManager, JobMode};
use crate::secrets::SecretsStore;
use crate::tools::tool::{NativeTool, ToolError, ToolOutput, require_str};

use super::SchedulerSlot;
use super::output::{error_output, success_output};

/// Tool for creating a new job.
///
/// When sandbox deps are injected (via `with_sandbox`), the tool automatically
/// delegates execution to a Docker container. Otherwise it creates an in-memory
/// job via the ContextManager. The LLM never needs to know the difference.
pub struct CreateJobTool {
    pub(super) context_manager: Arc<ContextManager>,
    /// Lazy scheduler for dispatching local (non-sandbox) jobs.
    pub(super) scheduler_slot: Option<SchedulerSlot>,
    pub(super) job_manager: Option<Arc<ContainerJobManager>>,
    pub(super) store: Option<Arc<dyn Database>>,
    /// Broadcast sender for job events (used to subscribe a monitor).
    pub(super) event_tx: Option<tokio::sync::broadcast::Sender<(Uuid, SseEvent)>>,
    /// Injection channel for pushing messages into the agent loop.
    pub(super) inject_tx: Option<tokio::sync::mpsc::Sender<IncomingMessage>>,
    /// Encrypted secrets store for validating credential grants.
    pub(super) secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
}

impl CreateJobTool {
    pub fn new(context_manager: Arc<ContextManager>) -> Self {
        Self {
            context_manager,
            scheduler_slot: None,
            job_manager: None,
            store: None,
            event_tx: None,
            inject_tx: None,
            secrets_store: None,
        }
    }

    /// Inject sandbox dependencies so `create_job` delegates to Docker containers.
    pub fn with_sandbox(
        mut self,
        job_manager: Arc<ContainerJobManager>,
        store: Option<Arc<dyn Database>>,
    ) -> Self {
        self.job_manager = Some(job_manager);
        self.store = store;
        self
    }

    /// Inject monitor dependencies so fire-and-forget jobs spawn a background
    /// monitor that forwards Claude Code output to the main agent loop.
    pub fn with_monitor_deps(
        mut self,
        event_tx: tokio::sync::broadcast::Sender<(Uuid, SseEvent)>,
        inject_tx: tokio::sync::mpsc::Sender<IncomingMessage>,
    ) -> Self {
        self.event_tx = Some(event_tx);
        self.inject_tx = Some(inject_tx);
        self
    }

    /// Inject a lazy scheduler slot for dispatching local (non-sandbox) jobs.
    pub fn with_scheduler_slot(mut self, slot: SchedulerSlot) -> Self {
        self.scheduler_slot = Some(slot);
        self
    }

    /// Inject secrets store for credential validation.
    pub fn with_secrets(mut self, secrets: Arc<dyn SecretsStore + Send + Sync>) -> Self {
        self.secrets_store = Some(secrets);
        self
    }

    pub fn sandbox_enabled(&self) -> bool {
        self.job_manager.is_some()
    }

    /// Update sandbox job status in DB (fire-and-forget for best-effort updates).
    ///
    /// This method uses `tokio::spawn` to make status updates intentionally
    /// fire-and-forget for non-terminal states (e.g., "creating", "running").
    /// These are recoverable on the next poll or after restart.
    /// For terminal states and pre-container failures, use `update_status_sync`.
    pub(super) fn update_status(&self, transition: StatusTransition) {
        let Some(store) = self.store.clone() else {
            return;
        };
        tokio::spawn(async move {
            apply_status_update(store, transition).await;
        });
    }

    /// Update sandbox job status synchronously (for terminal states and pre-container failures).
    ///
    /// This method awaits the status update to ensure durability before returning.
    /// Use for terminal states (completed, failed, cancelled) and pre-container
    /// failure transitions where the job state must be persisted before returning.
    pub(super) async fn update_status_sync(&self, transition: StatusTransition) {
        let Some(store) = self.store.clone() else {
            return;
        };
        apply_status_update(store, transition).await;
    }

    /// Execute via Scheduler (persists to DB + spawns worker), or fall back to
    /// ContextManager-only if the scheduler isn't available yet.
    async fn execute_local(
        &self,
        title: &str,
        description: &str,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        // Use the scheduler if available — creates in ContextManager, persists
        // to DB, transitions to InProgress, and spawns a worker. The new job
        // runs independently with its own Worker and LLM context (not inheriting
        // the parent conversation). MaxJobsExceeded is returned as error JSON
        // so the LLM can report it to the user.
        if let Some(ref slot) = self.scheduler_slot
            && let Some(ref scheduler) = *slot.read().await
        {
            return match scheduler
                .dispatch_job(crate::agent::scheduler::JobRequest {
                    user_id: &ctx.user_id,
                    title,
                    description,
                    metadata: None,
                })
                .await
            {
                Ok(job_id) => success_output(
                    serde_json::json!({
                        "job_id": job_id.to_string(),
                        "title": title,
                        "status": "in_progress",
                        "message": format!("Created and scheduled job '{}'", title)
                    }),
                    start,
                ),
                Err(e) => error_output(e.to_string(), start),
            };
        }

        // Fallback: ContextManager-only (scheduler not yet initialized).
        match self
            .context_manager
            .create_job_for_user(&ctx.user_id, title, description)
            .await
        {
            Ok(job_id) => success_output(
                serde_json::json!({
                    "job_id": job_id.to_string(),
                    "title": title,
                    "status": "pending",
                    "message": format!("Created job '{}' (not scheduled — scheduler unavailable)", title)
                }),
                start,
            ),
            Err(e) => error_output(e.to_string(), start),
        }
    }
}

/// Owned fields describing a sandbox job status transition.
pub(super) struct StatusTransition {
    job_id: Uuid,
    status: crate::db::SandboxJobStatus,
    success: Option<bool>,
    message: Option<String>,
    started_at: Option<chrono::DateTime<Utc>>,
    completed_at: Option<chrono::DateTime<Utc>>,
}

impl StatusTransition {
    /// Base transition to `status` with no outcome or timestamps.
    fn base(job_id: Uuid, status: &str) -> Self {
        Self {
            job_id,
            status: crate::db::SandboxJobStatus::from(status),
            success: None,
            message: None,
            started_at: None,
            completed_at: None,
        }
    }

    /// Transition to `running`, recording when the container started.
    pub(super) fn running(job_id: Uuid, started_at: chrono::DateTime<Utc>) -> Self {
        Self {
            started_at: Some(started_at),
            ..Self::base(job_id, "running")
        }
    }

    /// Transition to `completed` (success), recording the completion time.
    pub(super) fn completed(job_id: Uuid, completed_at: chrono::DateTime<Utc>) -> Self {
        Self {
            success: Some(true),
            completed_at: Some(completed_at),
            ..Self::base(job_id, "completed")
        }
    }

    /// Transition to `failed` with a failure message and completion time.
    pub(super) fn failed(
        job_id: Uuid,
        message: impl Into<String>,
        completed_at: chrono::DateTime<Utc>,
    ) -> Self {
        Self {
            success: Some(false),
            message: Some(message.into()),
            completed_at: Some(completed_at),
            ..Self::base(job_id, "failed")
        }
    }
}

/// Persist a sandbox job status transition, logging a warning on failure.
async fn apply_status_update(store: Arc<dyn Database>, transition: StatusTransition) {
    if let Err(e) = store
        .update_sandbox_job_status(SandboxJobStatusUpdate {
            id: transition.job_id,
            status: transition.status,
            success: transition.success,
            message: transition.message.as_deref(),
            started_at: transition.started_at,
            completed_at: transition.completed_at,
        })
        .await
    {
        tracing::warn!(
            job_id = %transition.job_id,
            "Failed to update sandbox job status: {}",
            e
        );
    }
}

impl NativeTool for CreateJobTool {
    fn name(&self) -> &str {
        "create_job"
    }

    fn description(&self) -> &str {
        if self.sandbox_enabled() {
            "Create and execute a job. The job runs in a sandboxed Docker container with its own \
             sub-agent that has shell, file read/write, list_dir, and apply_patch tools. Use this \
             whenever the user asks you to build, create, or work on something. The task \
             description should be detailed enough for the sub-agent to work independently. \
             Set wait=false to start immediately while continuing the conversation. Set mode \
             to 'claude_code' for complex software engineering tasks."
        } else {
            "Create a new job or task for the agent to work on. Use this when the user wants \
             you to do something substantial that should be tracked as a separate job."
        }
    }

    fn parameters_schema(&self) -> serde_json::Value {
        if self.sandbox_enabled() {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Clear description of what to accomplish"
                    },
                    "description": {
                        "type": "string",
                        "description": "Full description of what needs to be done"
                    },
                    "wait": {
                        "type": "boolean",
                        "description": "If true (default), wait for the container to complete and return results. \
                                        If false, start the container and return the job_id immediately."
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["worker", "claude_code"],
                        "description": "Execution mode. 'worker' (default) uses the Axinite sub-agent. \
                                        'claude_code' uses Claude Code CLI for full agentic software engineering."
                    },
                    "project_dir": {
                        "type": "string",
                        "description": "Path to an existing project directory to mount into the container. \
                                        Must be under ~/.axinite/projects/. If omitted, a fresh directory is created."
                    },
                    "credentials": {
                        "type": "object",
                        "description": "Map of secret names to env var names. Each secret must exist in the \
                                        secrets store (via 'axinite tool auth' or web UI). Example: \
                                        {\"github_token\": \"GITHUB_TOKEN\", \"npm_token\": \"NPM_TOKEN\"}",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["title", "description"]
            })
        } else {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "A short title for the job (max 100 chars)"
                    },
                    "description": {
                        "type": "string",
                        "description": "Full description of what needs to be done"
                    }
                },
                "required": ["title", "description"]
            })
        }
    }

    fn execution_timeout(&self) -> Duration {
        if self.sandbox_enabled() {
            // Sandbox polls for up to 10 min internally; give an extra 60s buffer.
            Duration::from_secs(660)
        } else {
            Duration::from_secs(30)
        }
    }

    fn rate_limit_config(&self) -> Option<crate::tools::tool::ToolRateLimitConfig> {
        Some(crate::tools::tool::ToolRateLimitConfig::new(5, 30))
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let title = require_str(&params, "title")?;

        let description = require_str(&params, "description")?;

        if self.sandbox_enabled() {
            let wait = params.get("wait").and_then(|v| v.as_bool()).unwrap_or(true);

            let mode = match params.get("mode").and_then(|v| v.as_str()) {
                Some("claude_code") => JobMode::ClaudeCode,
                _ => JobMode::Worker,
            };

            let explicit_dir = params
                .get("project_dir")
                .and_then(|v| v.as_str())
                .map(PathBuf::from);

            // Parse and validate credential grants
            let credential_grants = self.parse_credentials(&params, &ctx.user_id).await?;

            // Combine title and description into the task prompt for the sub-agent.
            let task = format!("{}\n\n{}", title, description);
            self.execute_sandbox(super::sandbox::SandboxExecution {
                task: &task,
                explicit_dir,
                wait,
                mode,
                credential_grants,
                ctx,
            })
            .await
        } else {
            self.execute_local(title, description, ctx).await
        }
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}
