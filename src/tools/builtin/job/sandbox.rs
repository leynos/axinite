//! Sandbox (Docker container) execution path for `CreateJobTool`.
//!
//! Creates the container job, optionally spawns a background monitor for
//! fire-and-forget jobs, and delegates waiting jobs to
//! [`super::container_polling`]. Job records and status writes live in
//! [`super::persistence`].

use std::path::PathBuf;

use chrono::Utc;
use uuid::Uuid;

use crate::context::JobContext;
use crate::orchestrator::auth::CredentialGrant;
use crate::orchestrator::job_manager::{ContainerJobManager, JobMode};
use crate::tools::tool::{ToolError, ToolOutput};

use super::CreateJobTool;
use super::persistence::StatusTransition;
use super::project_dir::resolve_project_dir;

/// Coordinates for one container job shared by the polling and completion
/// handlers: the job manager, job id, project paths, and the tool start time.
pub(super) struct ContainerJobView<'a> {
    pub(super) jm: &'a ContainerJobManager,
    pub(super) job_id: Uuid,
    pub(super) project_dir_str: &'a str,
    pub(super) browse_id: &'a str,
    pub(super) start: std::time::Instant,
}

impl ContainerJobView<'_> {
    /// Success output for a completed container job with the given message.
    pub(super) fn completed_output(&self, output: &str) -> ToolOutput {
        let result = serde_json::json!({
            "job_id": self.job_id.to_string(),
            "status": "completed",
            "output": output,
            "project_dir": self.project_dir_str,
            "browse_url": format!("/projects/{}", self.browse_id),
        });
        ToolOutput::success(result, self.start.elapsed())
    }
}

/// Inputs for launching a sandboxed (container) job execution.
///
/// Groups the task prompt, working-directory override, wait/mode flags,
/// credential grants, and job context so the entry point need not thread six
/// positional arguments.
pub(super) struct SandboxExecution<'a> {
    /// Combined task prompt handed to the sub-agent.
    pub task: &'a str,
    /// Optional explicit project directory; resolved automatically when absent.
    pub explicit_dir: Option<PathBuf>,
    /// Whether to poll to completion (`true`) or return after start.
    pub wait: bool,
    /// Execution mode selecting the container job runner.
    pub mode: JobMode,
    /// Credential grants forwarded to the container job.
    pub credential_grants: Vec<CredentialGrant>,
    /// Ambient job context (user identity and channel wiring).
    pub ctx: &'a JobContext,
}

/// Inputs for building a persisted sandbox job record.
pub(super) struct SandboxJobSpec<'a> {
    pub(super) task: &'a str,
    pub(super) job_id: Uuid,
    pub(super) user_id: crate::db::UserId,
    pub(super) project_dir_str: String,
    pub(super) credential_grants: &'a [CredentialGrant],
}

impl CreateJobTool {
    /// Execute via sandboxed Docker container.
    pub(super) async fn execute_sandbox(
        &self,
        exec: SandboxExecution<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let SandboxExecution {
            task,
            explicit_dir,
            wait,
            mode,
            credential_grants,
            ctx,
        } = exec;
        let start = std::time::Instant::now();
        let jm = self
            .job_manager
            .as_ref()
            .ok_or_else(|| ToolError::ExecutionFailed("sandbox deps required".to_string()))?;

        let job_id = Uuid::new_v4();
        let (project_dir, browse_id) = resolve_project_dir(explicit_dir, job_id)?;
        let project_dir_str = project_dir.display().to_string();

        // Build the job record and persist synchronously before creating the container.
        let record = Self::build_sandbox_job_record(SandboxJobSpec {
            task,
            job_id,
            user_id: crate::db::UserId::from(ctx.user_id.clone()),
            project_dir_str: project_dir_str.clone(),
            credential_grants: &credential_grants,
        });

        self.persist_sandbox_job(&record, mode).await?;

        // Create the container job with the pre-determined job_id.
        let _token = jm
            .create_job(job_id, task, Some(project_dir), mode, credential_grants)
            .await
            .map_err(|e| {
                self.update_status(StatusTransition::failed(job_id, e.to_string(), Utc::now()));
                ToolError::ExecutionFailed(format!("failed to create container: {}", e))
            })?;

        // Container started successfully.
        self.update_status(StatusTransition::running(job_id, Utc::now()));

        if !wait {
            // Spawn a background monitor that forwards Claude Code output
            // into the main agent loop.
            //
            // This monitor is intentionally fire-and-forget: its lifetime is
            // bound to the broadcast channel (etx) and the inject sender (itx).
            // When the broadcast sender is dropped during shutdown the
            // subscription closes and the monitor exits. Likewise, if the agent
            // loop stops consuming from inject_tx the send will fail and the
            // monitor terminates. No JoinHandle is retained.
            if let (Some(etx), Some(itx)) = (&self.event_tx, &self.inject_tx) {
                crate::agent::job_monitor::spawn_job_monitor(job_id, etx.subscribe(), itx.clone());
            }

            let result = serde_json::json!({
                "job_id": job_id.to_string(),
                "status": "started",
                "message": "Container started. Use job_events to check status or job_prompt to send follow-up instructions.",
                "project_dir": project_dir_str,
                "browse_url": format!("/projects/{}", browse_id),
            });
            return Ok(ToolOutput::success(result, start.elapsed()));
        }

        let view = ContainerJobView {
            jm,
            job_id,
            project_dir_str: &project_dir_str,
            browse_id: &browse_id,
            start,
        };
        self.await_container_completion(&view).await
    }
}
