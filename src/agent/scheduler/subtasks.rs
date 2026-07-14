//! Sub-task spawning and tool execution.
//!
//! Implements lightweight sub-tasks (parallel tool executions and background
//! computations) that run outside the full job lifecycle.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::oneshot;
use uuid::Uuid;

use super::{ScheduledSubtask, Scheduler};
use crate::agent::task::{Task, TaskContext, TaskOutput};
use crate::context::{ContextManager, JobContext, JobState};
use crate::error::{Error, JobError};
use crate::safety::SafetyLayer;
use crate::tools::{ApprovalContext, ToolRegistry};

impl Scheduler {
    /// Schedule a sub-task from within a worker.
    ///
    /// Sub-tasks are lightweight tasks that don't go through the full job lifecycle.
    /// They're used for parallel tool execution and background computations.
    ///
    /// Returns a oneshot receiver to get the result.
    pub async fn spawn_subtask(
        &self,
        parent_id: Uuid,
        task: Task,
    ) -> Result<oneshot::Receiver<Result<TaskOutput, Error>>, JobError> {
        let task_id = Uuid::new_v4();
        let (result_tx, result_rx) = oneshot::channel();

        let handle = match task {
            Task::Job { .. } => {
                // Jobs should go through schedule(), not spawn_subtask
                return Err(JobError::ContextError {
                    id: parent_id,
                    reason: "Use schedule() for Job tasks, not spawn_subtask()".to_string(),
                });
            }

            Task::ToolExec {
                parent_id: tool_parent_id,
                tool_name,
                params,
            } => {
                let tools = self.tools.clone();
                let context_manager = self.context_manager.clone();
                let safety = self.safety.clone();

                // TODO: propagate parent job's ApprovalContext here when subtasks
                // are used in autonomous/routine paths (currently only used in tests).
                tokio::spawn(async move {
                    let result = Self::execute_tool_task(
                        tools,
                        context_manager,
                        safety,
                        None,
                        tool_parent_id,
                        &tool_name,
                        params,
                    )
                    .await;

                    // Send result (ignore if receiver dropped)
                    let _ = result_tx.send(result);
                })
            }

            Task::Background { id: _, handler } => {
                let ctx = TaskContext::new(task_id).with_parent(parent_id);

                tokio::spawn(async move {
                    let result = handler.run(ctx).await;
                    let _ = result_tx.send(result);
                })
            }
        };

        // Track the subtask
        self.subtasks.write().await.insert(
            task_id,
            ScheduledSubtask {
                handle: tokio::spawn(async move {
                    // Wrap the handle to get its result
                    match handle.await {
                        Ok(()) => Err(Error::Job(JobError::ContextError {
                            id: task_id,
                            reason: "Subtask completed but result not captured".to_string(),
                        })),
                        Err(e) => Err(Error::Job(JobError::ContextError {
                            id: task_id,
                            reason: format!("Subtask panicked: {}", e),
                        })),
                    }
                }),
            },
        );

        // Cleanup task for subtask tracking
        let subtasks = Arc::clone(&self.subtasks);
        tokio::spawn(async move {
            loop {
                let finished = {
                    let subtasks_read = subtasks.read().await;
                    match subtasks_read.get(&task_id) {
                        Some(scheduled) => scheduled.handle.is_finished(),
                        None => true,
                    }
                };

                if finished {
                    subtasks.write().await.remove(&task_id);
                    break;
                }

                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });

        tracing::debug!(
            parent_id = %parent_id,
            task_id = %task_id,
            "Spawned subtask"
        );

        Ok(result_rx)
    }

    /// Schedule multiple tasks in parallel and wait for all to complete.
    ///
    /// Returns results in the same order as the input tasks.
    pub async fn spawn_batch(
        &self,
        parent_id: Uuid,
        tasks: Vec<Task>,
    ) -> Vec<Result<TaskOutput, Error>> {
        if tasks.is_empty() {
            return Vec::new();
        }

        let mut receivers = Vec::with_capacity(tasks.len());

        // Spawn all tasks
        for task in tasks {
            match self.spawn_subtask(parent_id, task).await {
                Ok(rx) => receivers.push(Some(rx)),
                Err(e) => {
                    // Store the error directly
                    receivers.push(None);
                    tracing::warn!(
                        parent_id = %parent_id,
                        error = %e,
                        "Failed to spawn subtask in batch"
                    );
                }
            }
        }

        // Collect results
        let mut results = Vec::with_capacity(receivers.len());
        for rx in receivers {
            let result = match rx {
                Some(receiver) => match receiver.await {
                    Ok(task_result) => task_result,
                    Err(_) => Err(Error::Job(JobError::ContextError {
                        id: parent_id,
                        reason: "Subtask channel closed unexpectedly".to_string(),
                    })),
                },
                None => Err(Error::Job(JobError::ContextError {
                    id: parent_id,
                    reason: "Subtask failed to spawn".to_string(),
                })),
            };
            results.push(result);
        }

        results
    }

    /// Execute a single tool as a subtask.
    ///
    /// Performs scheduler-specific checks (approval, cancellation) then
    /// delegates to the shared `execute_tool_with_safety` pipeline.
    pub(super) async fn execute_tool_task(
        tools: Arc<ToolRegistry>,
        context_manager: Arc<ContextManager>,
        safety: Arc<SafetyLayer>,
        approval_context: Option<ApprovalContext>,
        job_id: Uuid,
        tool_name: &str,
        params: serde_json::Value,
    ) -> Result<TaskOutput, Error> {
        let start = std::time::Instant::now();

        // Get the tool for approval check
        let tool = tools.get(tool_name).await.ok_or_else(|| {
            Error::Tool(crate::error::ToolError::NotFound {
                name: tool_name.to_string(),
            })
        })?;

        // Get job context
        let job_ctx: JobContext = context_manager.get_context(job_id).await?;
        if job_ctx.state == JobState::Cancelled {
            return Err(crate::error::ToolError::ExecutionFailed {
                name: tool_name.to_string(),
                reason: "Job is cancelled".to_string(),
            }
            .into());
        }

        // Scheduler-specific approval check
        let requirement = tool.requires_approval(&params);
        let blocked =
            ApprovalContext::is_blocked_or_default(&approval_context, tool_name, requirement);
        if blocked {
            return Err(crate::error::ToolError::AuthRequired {
                name: tool_name.to_string(),
            }
            .into());
        }

        // Delegate to shared tool execution pipeline
        let output_str = crate::tools::execute::execute_tool_with_safety(
            &tools, &safety, tool_name, &params, &job_ctx,
        )
        .await?;

        // Parse back to Value for TaskOutput; this should be infallible given
        // `execute_tool_with_safety` uses `serde_json::to_string_pretty`, but if it
        // ever fails we surface a clear error instead of silently changing types.
        let result_value: serde_json::Value = serde_json::from_str(&output_str).map_err(|e| {
            Error::Tool(crate::error::ToolError::ExecutionFailed {
                name: tool_name.to_string(),
                reason: format!("Failed to parse tool output as JSON: {}", e),
            })
        })?;

        Ok(TaskOutput::new(result_value, start.elapsed()))
    }
}
