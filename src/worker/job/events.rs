//! Job event logging, persistence, and SSE broadcasting.
//!
//! Non-terminal events are persisted fire-and-forget; terminal events are
//! persisted durably before broadcasting. Tool results are sanitized and
//! reported into both the reasoning context and the event stream.

use crate::agent::agentic_loop::truncate_for_preview;
use crate::channels::web::types::SseEvent;
use crate::context::JobState;
use crate::db::TerminalJobPersistence;
use crate::error::Error;
use crate::llm::{ReasoningContext, ToolSelection};
use crate::tools::execute::process_tool_result;

use super::Worker;

impl Worker {
    /// Fire-and-forget persistence and SSE broadcast for non-terminal job
    /// events only.
    ///
    /// `log_event` spawns the database write and does not await persistence.
    /// Terminal events must use `log_terminal_result_event`, which awaits
    /// persistence before broadcasting.
    pub(super) fn log_event(&self, event_type: &str, data: serde_json::Value) {
        let job_id = self.job_id;

        // Persist to DB
        if let Some(store) = self.store() {
            let store = store.clone();
            let et = event_type.to_string();
            let d = data.clone();
            tokio::spawn(async move {
                if let Err(e) = store
                    .save_job_event(job_id, crate::db::SandboxEventType::from(et), &d)
                    .await
                {
                    tracing::warn!("Failed to persist event for job {}: {}", job_id, e);
                }
            });
        }

        self.broadcast_event(event_type, &data);
    }

    /// Persist the terminal event and terminal status in one durable write.
    pub(super) async fn persist_terminal_result_and_status(
        &self,
        status: JobState,
        failure_reason: Option<&str>,
        event_type: &str,
        data: &serde_json::Value,
    ) -> Result<(), Error> {
        let job_id = self.job_id;
        if let Some(store) = self.store() {
            store
                .persist_terminal_result_and_status(TerminalJobPersistence {
                    job_id,
                    status,
                    failure_reason,
                    event_type: crate::db::SandboxEventType::from(event_type),
                    event_data: data,
                })
                .await
                .map_err(|e| crate::error::JobError::PersistenceError {
                    id: job_id,
                    reason: e.to_string(),
                })?;
        }

        self.broadcast_event(event_type, data);
        Ok(())
    }

    fn broadcast_event(&self, event_type: &str, data: &serde_json::Value) {
        // Broadcast SSE for live web UI updates
        if let Some(ref tx) = self.deps.sse_tx {
            let job_id_str = self.job_id.to_string();
            let event = match event_type {
                "message" => Some(SseEvent::JobMessage {
                    job_id: job_id_str,
                    role: data
                        .get("role")
                        .and_then(|v| v.as_str())
                        .unwrap_or("assistant")
                        .to_string(),
                    content: data
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }),
                "tool_use" => Some(SseEvent::JobToolUse {
                    job_id: job_id_str,
                    tool_name: data
                        .get("tool_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    input: data
                        .get("input")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                }),
                "tool_result" => Some(SseEvent::JobToolResult {
                    job_id: job_id_str,
                    tool_name: data
                        .get("tool_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    output: data
                        .get("output")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }),
                "status" => Some(SseEvent::JobStatus {
                    job_id: job_id_str,
                    message: data
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }),
                "result" => Some(SseEvent::JobResult {
                    job_id: job_id_str,
                    status: data
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("completed")
                        .to_string(),
                    session_id: data
                        .get("session_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                }),
                _ => None,
            };
            if let Some(event) = event {
                let _ = tx.send(event);
            }
        }
    }

    /// Process a tool execution result and add it to the reasoning context.
    pub(super) async fn process_tool_result_job(
        &self,
        reason_ctx: &mut ReasoningContext,
        selection: &ToolSelection,
        result: Result<String, Error>,
    ) -> Result<(), Error> {
        self.log_event(
            "tool_use",
            serde_json::json!({
                "tool_name": selection.tool_name,
                "input": truncate_for_preview(
                    &selection.parameters.to_string(), 500),
            }),
        );

        // Use shared result processing for sanitize → wrap → ChatMessage.
        // The wrapped content (XML tags) goes into reason_ctx for the LLM.
        // The raw sanitized content goes into events/SSE for human-readable UI.
        let (_wrapped, message) = process_tool_result(
            &self.deps.safety,
            &selection.tool_name,
            &selection.tool_call_id,
            &result,
        );
        reason_ctx.messages.push(message);

        match &result {
            Ok(raw_output) => {
                let sanitized = self
                    .deps
                    .safety
                    .sanitize_tool_output(&selection.tool_name, raw_output);
                self.log_event(
                    "tool_result",
                    serde_json::json!({
                        "tool_name": selection.tool_name,
                        "success": true,
                        "output": truncate_for_preview(&sanitized.content, 500),
                    }),
                );
                Ok(())
            }
            Err(e) => {
                tracing::warn!(
                    "Tool {} failed for job {}: {}",
                    selection.tool_name,
                    self.job_id,
                    e
                );

                // Record failure for self-repair tracking
                if let Some(store) = self.store() {
                    let store = store.clone();
                    let tool_name = selection.tool_name.clone();
                    let error_msg = e.to_string();
                    tokio::spawn(async move {
                        if let Err(db_err) = store.record_tool_failure(&tool_name, &error_msg).await
                        {
                            tracing::warn!("Failed to record tool failure: {}", db_err);
                        }
                    });
                }

                self.log_event(
                    "tool_result",
                    serde_json::json!({
                        "tool_name": selection.tool_name,
                        "success": false,
                        "output": truncate_for_preview(&format!("Error: {}", e), 500),
                    }),
                );

                Ok(())
            }
        }
    }
}
