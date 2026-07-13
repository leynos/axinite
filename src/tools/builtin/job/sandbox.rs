//! Sandbox (Docker container) execution path for `CreateJobTool`.
//!
//! Builds and persists the sandbox job record, creates the container job,
//! optionally spawns a background monitor for fire-and-forget jobs, and polls
//! container state to completion for waiting jobs.

use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use uuid::Uuid;

use crate::context::JobContext;
use crate::db::SandboxJobStatusUpdate;
use crate::history::SandboxJobRecord;
use crate::orchestrator::auth::CredentialGrant;
use crate::orchestrator::job_manager::{ContainerJobManager, JobMode};
use crate::tools::tool::{ToolError, ToolOutput};

use super::CreateJobTool;
use super::project_dir::resolve_project_dir;

impl CreateJobTool {
    /// Build a sandbox job record from the given parameters.
    fn build_sandbox_job_record(
        task: &str,
        job_id: Uuid,
        user_id: crate::db::UserId,
        project_dir_str: String,
        credential_grants: &[CredentialGrant],
    ) -> SandboxJobRecord {
        // Serialize credential grants so restarts can reload them.
        let credential_grants_json = match serde_json::to_string(credential_grants) {
            Ok(json) => json,
            Err(e) => {
                tracing::warn!(
                    "Failed to serialize credential grants for job {}: {}. \
                     Grants will not survive a restart.",
                    job_id,
                    e
                );
                String::from("[]")
            }
        };

        SandboxJobRecord {
            id: job_id,
            task: task.to_string(),
            status: "creating".to_string(),
            user_id,
            project_dir: project_dir_str,
            success: None,
            failure_reason: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            credential_grants_json,
        }
    }

    /// Persist sandbox job record and optional mode to the database.
    async fn persist_sandbox_job(
        &self,
        record: &SandboxJobRecord,
        mode: JobMode,
    ) -> Result<(), ToolError> {
        let Some(store) = self.store.clone() else {
            return Ok(());
        };

        store
            .save_sandbox_job(record)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to persist job: {}", e)))?;

        if mode == JobMode::ClaudeCode
            && let Err(e) = store
                .update_sandbox_job_mode(record.id, crate::db::SandboxMode::ClaudeCode)
                .await
        {
            // Synchronously update status to failed before returning error
            // (don't use fire-and-forget update_status here)
            let status = crate::db::SandboxJobStatus::from("failed");
            let _ = store
                .update_sandbox_job_status(SandboxJobStatusUpdate {
                    id: record.id,
                    status,
                    success: Some(false),
                    message: Some(&e.to_string()),
                    started_at: None,
                    completed_at: Some(Utc::now()),
                })
                .await;
            return Err(ToolError::ExecutionFailed(format!(
                "failed to persist job mode: {}",
                e
            )));
        }

        Ok(())
    }

    /// Handle a stopped container, returning success or failure output.
    async fn handle_stopped_container(
        &self,
        handle: &crate::orchestrator::job_manager::ContainerHandle,
        jm: &ContainerJobManager,
        job_id: Uuid,
        project_dir_str: &str,
        browse_id: &str,
        start: std::time::Instant,
    ) -> Result<ToolOutput, ToolError> {
        let message = handle
            .completion_result
            .as_ref()
            .and_then(|r| r.message.clone())
            .unwrap_or_else(|| "Container job completed".to_string());
        let success = handle
            .completion_result
            .as_ref()
            .map(|r| r.success)
            .unwrap_or(true);
        jm.cleanup_job(job_id).await;

        let finished_at = Utc::now();
        if success {
            self.update_status_sync(
                job_id,
                "completed",
                Some(true),
                None,
                None,
                Some(finished_at),
            )
            .await;
            let result = serde_json::json!({
                "job_id": job_id.to_string(),
                "status": "completed",
                "output": message,
                "project_dir": project_dir_str,
                "browse_url": format!("/projects/{}", browse_id),
            });
            Ok(ToolOutput::success(result, start.elapsed()))
        } else {
            self.update_status_sync(
                job_id,
                "failed",
                Some(false),
                Some(message.clone()),
                None,
                Some(finished_at),
            )
            .await;
            Err(ToolError::ExecutionFailed(format!(
                "container job failed: {}",
                message
            )))
        }
    }

    /// Handle a failed container, returning an error.
    async fn handle_failed_container(
        &self,
        handle: &crate::orchestrator::job_manager::ContainerHandle,
        jm: &ContainerJobManager,
        job_id: Uuid,
    ) -> Result<ToolOutput, ToolError> {
        let message = handle
            .completion_result
            .as_ref()
            .and_then(|r| r.message.clone())
            .unwrap_or_else(|| "unknown failure".to_string());
        jm.cleanup_job(job_id).await;
        self.update_status_sync(
            job_id,
            "failed",
            Some(false),
            Some(message.clone()),
            None,
            Some(Utc::now()),
        )
        .await;
        Err(ToolError::ExecutionFailed(format!(
            "container job failed: {}",
            message
        )))
    }

    /// Handle the case where the container handle is no longer present.
    ///
    /// An absent handle is treated as a silent successful completion — the
    /// container finished before we could observe a terminal state.
    async fn handle_missing_container(
        &self,
        job_id: Uuid,
        project_dir_str: &str,
        browse_id: &str,
        start: std::time::Instant,
    ) -> Result<ToolOutput, ToolError> {
        self.update_status_sync(
            job_id,
            "completed",
            Some(true),
            None,
            None,
            Some(Utc::now()),
        )
        .await;
        let result = serde_json::json!({
            "job_id": job_id.to_string(),
            "status": "completed",
            "output": "Container job completed",
            "project_dir": project_dir_str,
            "browse_url": format!("/projects/{}", browse_id),
        });
        Ok(ToolOutput::success(result, start.elapsed()))
    }

    /// Poll container state until completion or timeout.
    async fn await_container_completion(
        &self,
        jm: &ContainerJobManager,
        job_id: Uuid,
        project_dir_str: &str,
        browse_id: &str,
        start: std::time::Instant,
    ) -> Result<ToolOutput, ToolError> {
        let timeout = Duration::from_secs(600);
        let poll_interval = Duration::from_secs(2);
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if tokio::time::Instant::now() > deadline {
                let _ = jm.stop_job(job_id).await;
                jm.cleanup_job(job_id).await;
                self.update_status_sync(
                    job_id,
                    "failed",
                    Some(false),
                    Some("Timed out (10 minutes)".to_string()),
                    None,
                    Some(Utc::now()),
                )
                .await;
                return Err(ToolError::ExecutionFailed(
                    "container execution timed out (10 minutes)".to_string(),
                ));
            }

            match jm.get_handle(job_id).await {
                Some(handle) => match handle.state {
                    crate::orchestrator::job_manager::ContainerState::Running
                    | crate::orchestrator::job_manager::ContainerState::Creating => {
                        tokio::time::sleep(poll_interval).await;
                    }
                    crate::orchestrator::job_manager::ContainerState::Stopped => {
                        return self
                            .handle_stopped_container(
                                &handle,
                                jm,
                                job_id,
                                project_dir_str,
                                browse_id,
                                start,
                            )
                            .await;
                    }
                    crate::orchestrator::job_manager::ContainerState::Failed => {
                        return self.handle_failed_container(&handle, jm, job_id).await;
                    }
                },
                None => {
                    return self
                        .handle_missing_container(job_id, project_dir_str, browse_id, start)
                        .await;
                }
            }
        }
    }

    /// Execute via sandboxed Docker container.
    pub(super) async fn execute_sandbox(
        &self,
        task: &str,
        explicit_dir: Option<PathBuf>,
        wait: bool,
        mode: JobMode,
        credential_grants: Vec<CredentialGrant>,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let jm = self
            .job_manager
            .as_ref()
            .ok_or_else(|| ToolError::ExecutionFailed("sandbox deps required".to_string()))?;

        let job_id = Uuid::new_v4();
        let (project_dir, browse_id) = resolve_project_dir(explicit_dir, job_id)?;
        let project_dir_str = project_dir.display().to_string();

        // Build the job record and persist synchronously before creating the container.
        let record = Self::build_sandbox_job_record(
            task,
            job_id,
            crate::db::UserId::from(ctx.user_id.clone()),
            project_dir_str.clone(),
            &credential_grants,
        );

        self.persist_sandbox_job(&record, mode).await?;

        // Create the container job with the pre-determined job_id.
        let _token = jm
            .create_job(job_id, task, Some(project_dir), mode, credential_grants)
            .await
            .map_err(|e| {
                self.update_status(
                    job_id,
                    "failed",
                    Some(false),
                    Some(e.to_string()),
                    None,
                    Some(Utc::now()),
                );
                ToolError::ExecutionFailed(format!("failed to create container: {}", e))
            })?;

        // Container started successfully.
        let now = Utc::now();
        self.update_status(job_id, "running", None, None, Some(now), None);

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

        self.await_container_completion(jm, job_id, &project_dir_str, &browse_id, start)
            .await
    }
}
