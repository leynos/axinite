//! Job listing, status inspection, and cancellation tools.
//!
//! `ListJobsTool`, `JobStatusTool`, and `CancelJobTool` operate on in-memory
//! job contexts via the `ContextManager`, scoped to the requesting user.

use std::sync::Arc;

use crate::context::{ContextManager, JobContext, JobState};
use crate::tools::tool::{ApprovalRequirement, NativeTool, ToolError, ToolOutput, require_str};

use super::resolve_job_id;

/// Tool for listing jobs.
pub struct ListJobsTool {
    context_manager: Arc<ContextManager>,
}

impl ListJobsTool {
    pub fn new(context_manager: Arc<ContextManager>) -> Self {
        Self { context_manager }
    }
}

impl NativeTool for ListJobsTool {
    fn name(&self) -> &str {
        "list_jobs"
    }

    fn description(&self) -> &str {
        "List all jobs or filter by status. Shows job IDs, titles, and current status."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "filter": {
                    "type": "string",
                    "description": "Filter by status: 'active', 'completed', 'failed', 'all' (default: 'all')",
                    "enum": ["active", "completed", "failed", "all"]
                }
            }
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let filter = params
            .get("filter")
            .and_then(|v| v.as_str())
            .unwrap_or("all");

        let job_ids = match filter {
            "active" => self.context_manager.active_jobs_for(&ctx.user_id).await,
            _ => self.context_manager.all_jobs_for(&ctx.user_id).await,
        };

        let mut jobs = Vec::new();
        for job_id in job_ids {
            if let Ok(ctx) = self.context_manager.get_context(job_id).await {
                let include = match filter {
                    "completed" => ctx.state == JobState::Completed,
                    "failed" => ctx.state == JobState::Failed,
                    "active" => ctx.state.is_active(),
                    _ => true,
                };

                if include {
                    jobs.push(serde_json::json!({
                        "job_id": job_id.to_string(),
                        "title": ctx.title,
                        "status": format!("{:?}", ctx.state),
                        "created_at": ctx.created_at.to_rfc3339()
                    }));
                }
            }
        }

        let summary = self.context_manager.summary_for(&ctx.user_id).await;

        let result = serde_json::json!({
            "jobs": jobs,
            "summary": {
                "total": summary.total,
                "pending": summary.pending,
                "in_progress": summary.in_progress,
                "completed": summary.completed,
                "failed": summary.failed
            }
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

/// Tool for checking job status.
pub struct JobStatusTool {
    context_manager: Arc<ContextManager>,
}

impl JobStatusTool {
    pub fn new(context_manager: Arc<ContextManager>) -> Self {
        Self { context_manager }
    }
}

impl NativeTool for JobStatusTool {
    fn name(&self) -> &str {
        "job_status"
    }

    fn description(&self) -> &str {
        "Check the status and details of a specific job by its ID."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "job_id": {
                    "type": "string",
                    "description": "The job ID (full UUID or short prefix, e.g. 'f2854dd8')"
                }
            },
            "required": ["job_id"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let requester_id = ctx.user_id.clone();

        let job_id_str = require_str(&params, "job_id")?;
        let job_id = resolve_job_id(job_id_str, &self.context_manager).await?;

        match self.context_manager.get_context(job_id).await {
            Ok(job_ctx) => {
                if job_ctx.user_id != requester_id {
                    let result = serde_json::json!({
                        "error": "Job not found".to_string()
                    });
                    return Ok(ToolOutput::success(result, start.elapsed()));
                }
                let result = serde_json::json!({
                    "job_id": job_id.to_string(),
                    "title": job_ctx.title,
                    "description": job_ctx.description,
                    "status": format!("{:?}", job_ctx.state),
                    "created_at": job_ctx.created_at.to_rfc3339(),
                    "started_at": job_ctx.started_at.map(|t| t.to_rfc3339()),
                    "completed_at": job_ctx.completed_at.map(|t| t.to_rfc3339()),
                    "actual_cost": job_ctx.actual_cost.to_string()
                });
                Ok(ToolOutput::success(result, start.elapsed()))
            }
            Err(e) => {
                let result = serde_json::json!({
                    "error": format!("Job not found: {}", e)
                });
                Ok(ToolOutput::success(result, start.elapsed()))
            }
        }
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

/// Tool for canceling a job.
pub struct CancelJobTool {
    context_manager: Arc<ContextManager>,
}

impl CancelJobTool {
    pub fn new(context_manager: Arc<ContextManager>) -> Self {
        Self { context_manager }
    }
}

impl NativeTool for CancelJobTool {
    fn name(&self) -> &str {
        "cancel_job"
    }

    fn description(&self) -> &str {
        "Cancel a running or pending job. The job will be marked as cancelled and stopped."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "job_id": {
                    "type": "string",
                    "description": "The job ID (full UUID or short prefix, e.g. 'f2854dd8')"
                }
            },
            "required": ["job_id"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();
        let requester_id = ctx.user_id.clone();

        let job_id_str = require_str(&params, "job_id")?;
        let job_id = resolve_job_id(job_id_str, &self.context_manager).await?;

        // Transition to cancelled state
        match self
            .context_manager
            .update_context(job_id, |ctx| {
                if ctx.user_id != requester_id {
                    return Err("Job not found".to_string());
                }
                ctx.transition_to(JobState::Cancelled, Some("Cancelled by user".to_string()))
            })
            .await
        {
            Ok(Ok(())) => {
                let result = serde_json::json!({
                    "job_id": job_id.to_string(),
                    "status": "cancelled",
                    "message": "Job cancelled successfully"
                });
                Ok(ToolOutput::success(result, start.elapsed()))
            }
            Ok(Err(reason)) => {
                let result = serde_json::json!({
                    "error": format!("Cannot cancel job: {}", reason)
                });
                Ok(ToolOutput::success(result, start.elapsed()))
            }
            Err(e) => {
                let result = serde_json::json!({
                    "error": format!("Job not found: {}", e)
                });
                Ok(ToolOutput::success(result, start.elapsed()))
            }
        }
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}
