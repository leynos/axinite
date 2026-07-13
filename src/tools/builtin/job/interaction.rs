//! Tools for interacting with running sandbox jobs.
//!
//! `JobEventsTool` reads a sandbox job's event log from the database;
//! `JobPromptTool` queues follow-up prompts for a running Claude Code job.
//! Both verify job ownership before acting.

use std::sync::Arc;

use uuid::Uuid;

use crate::context::{ContextManager, JobContext};
use crate::db::Database;
use crate::tools::tool::{ApprovalRequirement, NativeTool, ToolError, ToolOutput};

use super::resolve_job_id;

/// Tool for reading sandbox job event logs.
///
/// Lets the main agent inspect what a running (or completed) container job has
/// been doing: messages, tool calls, results, status changes, etc.
///
/// Events are streamed from the sandbox worker into the database via the
/// orchestrator's event pipeline. This tool queries them with a DB-level
/// `LIMIT` (default 50, configurable via the `limit` parameter) so the
/// agent sees the most recent activity without loading the full history.
pub struct JobEventsTool {
    store: Arc<dyn Database>,
    context_manager: Arc<ContextManager>,
}

impl JobEventsTool {
    pub fn new(store: Arc<dyn Database>, context_manager: Arc<ContextManager>) -> Self {
        Self {
            store,
            context_manager,
        }
    }
}

impl NativeTool for JobEventsTool {
    fn name(&self) -> &str {
        "job_events"
    }

    fn description(&self) -> &str {
        "Read the event log for a sandbox job. Shows messages, tool calls, results, \
         and status changes from the container. Use this to check what Claude Code \
         or a worker sub-agent has been doing."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "job_id": {
                    "type": "string",
                    "description": "The job ID (full UUID or short prefix, e.g. 'f2854dd8')"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of events to return (default 50, most recent)"
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

        let job_id_str = params
            .get("job_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParameters("missing 'job_id' parameter".into()))?;

        let job_id = resolve_job_id(job_id_str, &self.context_manager).await?;

        // Verify the caller owns this job. A missing context is treated as
        // unauthorized to prevent leaking events after process restarts.
        let job_ctx = self
            .context_manager
            .get_context(job_id)
            .await
            .map_err(|_| {
                ToolError::ExecutionFailed(format!(
                    "job {} not found or context unavailable",
                    job_id
                ))
            })?;

        if job_ctx.user_id != ctx.user_id {
            return Err(ToolError::ExecutionFailed(format!(
                "job {} does not belong to current user",
                job_id
            )));
        }

        const MAX_EVENT_LIMIT: i64 = 1000;
        let limit = params
            .get("limit")
            .and_then(|v| v.as_i64())
            .unwrap_or(50)
            .clamp(1, MAX_EVENT_LIMIT);

        let events = self
            .store
            .list_job_events(job_id, None, Some(limit))
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to load job events: {}", e)))?;

        let recent: Vec<serde_json::Value> = events
            .iter()
            .map(|ev| {
                serde_json::json!({
                    "event_type": ev.event_type,
                    "data": ev.data,
                    "created_at": ev.created_at.to_rfc3339(),
                })
            })
            .collect();

        let result = serde_json::json!({
            "job_id": job_id.to_string(),
            "total_events": events.len(),
            "returned": recent.len(),
            "events": recent,
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        true
    }
}

/// Tool for sending follow-up prompts to a running Claude Code sandbox job.
///
/// The prompt is queued in an in-memory `PromptQueue` (a broadcast channel
/// shared with the web gateway). The Claude Code bridge inside the container
/// polls for queued prompts between turns and feeds them into the next
/// `claude --resume` invocation, enabling interactive multi-turn sessions
/// with long-running sandbox jobs.
pub struct JobPromptTool {
    prompt_queue: PromptQueue,
    context_manager: Arc<ContextManager>,
}

/// Type alias matching `crate::channels::web::server::PromptQueue`.
pub type PromptQueue = Arc<
    tokio::sync::Mutex<
        std::collections::HashMap<
            Uuid,
            std::collections::VecDeque<crate::orchestrator::api::PendingPrompt>,
        >,
    >,
>;

impl JobPromptTool {
    pub fn new(prompt_queue: PromptQueue, context_manager: Arc<ContextManager>) -> Self {
        Self {
            prompt_queue,
            context_manager,
        }
    }
}

impl NativeTool for JobPromptTool {
    fn name(&self) -> &str {
        "job_prompt"
    }

    fn description(&self) -> &str {
        "Send a follow-up prompt to a running Claude Code sandbox job. The prompt is \
         queued and delivered on the next poll cycle. Use this to give the sub-agent \
         additional instructions, answer its questions, or tell it to wrap up."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "job_id": {
                    "type": "string",
                    "description": "The job ID (full UUID or short prefix, e.g. 'f2854dd8')"
                },
                "content": {
                    "type": "string",
                    "description": "The follow-up prompt text to send"
                },
                "done": {
                    "type": "boolean",
                    "description": "If true, signals the sub-agent that no more prompts are coming \
                                    and it should finish up. Default false."
                }
            },
            "required": ["job_id", "content"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let job_id_str = params
            .get("job_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParameters("missing 'job_id' parameter".into()))?;

        let job_id = resolve_job_id(job_id_str, &self.context_manager).await?;

        // Verify the caller owns this job. A missing context is treated as
        // unauthorized to prevent sending prompts to jobs after process restarts.
        let job_ctx = self
            .context_manager
            .get_context(job_id)
            .await
            .map_err(|_| {
                ToolError::ExecutionFailed(format!(
                    "job {} not found or context unavailable",
                    job_id
                ))
            })?;

        if job_ctx.user_id != ctx.user_id {
            return Err(ToolError::ExecutionFailed(format!(
                "job {} does not belong to current user",
                job_id
            )));
        }

        let content = params
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidParameters("missing 'content' parameter".into()))?;

        let done = params
            .get("done")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let prompt = crate::orchestrator::api::PendingPrompt {
            content: content.to_string(),
            done,
        };

        {
            let mut queue = self.prompt_queue.lock().await;
            queue.entry(job_id).or_default().push_back(prompt);
        }

        let result = serde_json::json!({
            "job_id": job_id.to_string(),
            "status": "queued",
            "message": "Prompt queued",
            "done": done,
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}
