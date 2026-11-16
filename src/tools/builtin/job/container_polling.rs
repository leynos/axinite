//! Container polling for the sandbox execution path.
//!
//! This module watches container state and dispatches each terminal outcome:
//! success, failure, a container handle that disappeared, or a polling
//! deadline. Each handler delegates status writes to [`super::persistence`].

use std::time::Duration;

use chrono::Utc;

use crate::orchestrator::job_manager::{ContainerHandle, ContainerState};
use crate::tools::tool::{ToolError, ToolOutput};

use super::CreateJobTool;
use super::persistence::StatusTransition;
use super::sandbox::ContainerJobView;

impl CreateJobTool {
    /// Handle a stopped container, returning success or failure output.
    ///
    /// Only an explicit, successful worker result counts as success. A
    /// container can reach [`ContainerState::Stopped`] without a result when
    /// it is stopped by the web cancel handler or removed by the reaper, and
    /// such a job did not complete successfully.
    async fn handle_stopped_container(
        &self,
        handle: &ContainerHandle,
        view: &ContainerJobView<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let (success, message) = match handle.completion_result.as_ref() {
            Some(result) => {
                let message = result.message.clone().unwrap_or_else(|| {
                    if result.success {
                        "Container job completed".to_string()
                    } else {
                        "unknown failure".to_string()
                    }
                });
                (result.success, message)
            }
            None => (
                false,
                "Container stopped without a completion result".to_string(),
            ),
        };
        view.jm.cleanup_job(view.job_id).await;

        let finished_at = Utc::now();
        if success {
            self.update_status_sync(StatusTransition::completed(view.job_id, finished_at))
                .await;
            Ok(view.completed_output(&message))
        } else {
            self.update_status_sync(StatusTransition::failed(
                view.job_id,
                message.clone(),
                finished_at,
            ))
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
        handle: &ContainerHandle,
        view: &ContainerJobView<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let message = handle
            .completion_result
            .as_ref()
            .and_then(|r| r.message.clone())
            .unwrap_or_else(|| "unknown failure".to_string());
        view.jm.cleanup_job(view.job_id).await;
        self.update_status_sync(StatusTransition::failed(
            view.job_id,
            message.clone(),
            Utc::now(),
        ))
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
        view: &ContainerJobView<'_>,
    ) -> Result<ToolOutput, ToolError> {
        self.update_status_sync(StatusTransition::completed(view.job_id, Utc::now()))
            .await;
        Ok(view.completed_output("Container job completed"))
    }

    /// Stop and clean up a container that exceeded the polling deadline,
    /// persisting the failed status before returning the timeout error.
    async fn handle_timed_out_container(
        &self,
        view: &ContainerJobView<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let _ = view.jm.stop_job(view.job_id).await;
        view.jm.cleanup_job(view.job_id).await;
        self.update_status_sync(StatusTransition::failed(
            view.job_id,
            "Timed out (10 minutes)",
            Utc::now(),
        ))
        .await;
        Err(ToolError::ExecutionFailed(
            "container execution timed out (10 minutes)".to_string(),
        ))
    }

    /// Poll container state until completion or timeout.
    pub(super) async fn await_container_completion(
        &self,
        view: &ContainerJobView<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let timeout = Duration::from_secs(600);
        let poll_interval = Duration::from_secs(2);
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if tokio::time::Instant::now() > deadline {
                return self.handle_timed_out_container(view).await;
            }

            let Some(handle) = view.jm.get_handle(view.job_id).await else {
                return self.handle_missing_container(view).await;
            };

            match handle.state {
                ContainerState::Running | ContainerState::Creating => {
                    tokio::time::sleep(poll_interval).await;
                }
                ContainerState::Stopped => {
                    return self.handle_stopped_container(&handle, view).await;
                }
                ContainerState::Failed => {
                    return self.handle_failed_container(&handle, view).await;
                }
            }
        }
    }
}
