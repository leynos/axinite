//! Job persistence for the sandbox execution path.
//!
//! This module builds sandbox job records and writes job status to the
//! `Database`, including the status transitions used by the sandbox lifecycle.

use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::db::{Database, SandboxJobStatusUpdate};
use crate::history::SandboxJobRecord;
use crate::orchestrator::job_manager::JobMode;
use crate::tools::tool::ToolError;

use super::CreateJobTool;
use super::sandbox::SandboxJobSpec;

impl CreateJobTool {
    /// Build a sandbox job record from the given parameters.
    pub(super) fn build_sandbox_job_record(spec: SandboxJobSpec<'_>) -> SandboxJobRecord {
        let SandboxJobSpec {
            task,
            job_id,
            user_id,
            project_dir_str,
            credential_grants,
        } = spec;
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
    pub(super) async fn persist_sandbox_job(
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
