//! Loop-delegate implementation for worker containers.
//!
//! This module bridges the agentic loop to the worker HTTP API, posting events,
//! polling follow-up prompts, executing tools, and pacing iterations so the
//! runtime remains responsive.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::WorkerHttpClient;
use crate::agent::agentic_loop::{
    LoopDelegate, LoopOutcome, LoopSignal, TextAction, truncate_for_preview,
};
use crate::context::JobContext;
use crate::llm::{ChatMessage, Reasoning, ReasoningContext};
use crate::safety::SafetyLayer;
use crate::tools::ToolRegistry;
use crate::tools::execute::{execute_tool_simple, process_tool_result};
use crate::worker::api::{JobEventPayload, StatusUpdate, WorkerState};

/// Container delegate: implements `LoopDelegate` for the Docker container context.
///
/// Tools execute sequentially. Events are posted to the orchestrator via HTTP.
/// Completion is detected via `llm_signals_completion()`.
pub(super) struct ContainerDelegate {
    pub(super) client: Arc<WorkerHttpClient>,
    pub(super) safety: Arc<SafetyLayer>,
    pub(super) tools: Arc<ToolRegistry>,
    pub(super) extra_env: Arc<HashMap<String, String>>,
    /// Tracks the last successful tool output for the final response.
    pub(super) last_output: Mutex<String>,
    /// Tracks the current iteration so `CompletionReport` can include accurate counts.
    pub(super) iteration_tracker: Arc<Mutex<u32>>,
}

impl ContainerDelegate {
    pub(super) async fn post_event(&self, event_type: &str, data: serde_json::Value) {
        self.client
            .post_event(&JobEventPayload {
                event_type: event_type.to_string(),
                data,
            })
            .await;
    }

    async fn poll_and_inject_prompt(&self, reason_ctx: &mut ReasoningContext) {
        match self.client.poll_prompt().await {
            Ok(Some(prompt)) => {
                tracing::info!(
                    "Received follow-up prompt: {}",
                    truncate_for_preview(&prompt.content, 100)
                );
                self.post_event(
                    "message",
                    serde_json::json!({
                        "role": "user",
                        "content": truncate_for_preview(&prompt.content, 2000),
                    }),
                )
                .await;
                reason_ctx.messages.push(ChatMessage::user(&prompt.content));
            }
            Ok(None) => {}
            Err(e) => {
                tracing::debug!("Failed to poll for prompt: {}", e);
            }
        }
    }
}

#[async_trait]
impl LoopDelegate for ContainerDelegate {
    async fn check_signals(&self) -> LoopSignal {
        LoopSignal::Continue
    }

    async fn before_llm_call(
        &self,
        reason_ctx: &mut ReasoningContext,
        iteration: usize,
    ) -> Option<LoopOutcome> {
        let iteration = iteration as u32;
        *self.iteration_tracker.lock().await = iteration;

        if iteration % 5 == 1 {
            let _ = self
                .client
                .report_status(&StatusUpdate::new(
                    WorkerState::InProgress,
                    Some(format!("Iteration {}", iteration)),
                    iteration,
                ))
                .await;
        }

        self.poll_and_inject_prompt(reason_ctx).await;
        reason_ctx.available_tools = self.tools.tool_definitions().await;

        None
    }

    async fn call_llm(
        &self,
        reasoning: &Reasoning,
        reason_ctx: &mut ReasoningContext,
        _iteration: usize,
    ) -> Result<crate::llm::RespondOutput, crate::error::Error> {
        reasoning
            .respond_with_tools(reason_ctx)
            .await
            .map_err(Into::into)
    }

    async fn handle_text_response(
        &self,
        text: &str,
        reason_ctx: &mut ReasoningContext,
    ) -> TextAction {
        self.post_event(
            "message",
            serde_json::json!({
                "role": "assistant",
                "content": truncate_for_preview(text, 2000),
            }),
        )
        .await;

        if crate::util::llm_signals_completion(text) {
            let last = self.last_output.lock().await;
            let output = if last.is_empty() {
                text.to_string()
            } else {
                last.clone()
            };
            return TextAction::Return(LoopOutcome::Response(output));
        }

        reason_ctx.messages.push(ChatMessage::assistant(text));
        TextAction::Continue
    }

    async fn execute_tool_calls(
        &self,
        tool_calls: Vec<crate::llm::ToolCall>,
        content: Option<String>,
        reason_ctx: &mut ReasoningContext,
    ) -> Result<Option<LoopOutcome>, crate::error::Error> {
        if let Some(ref text) = content {
            self.post_event(
                "message",
                serde_json::json!({
                    "role": "assistant",
                    "content": truncate_for_preview(text, 2000),
                }),
            )
            .await;
        }

        reason_ctx
            .messages
            .push(ChatMessage::assistant_with_tool_calls(
                content,
                tool_calls.clone(),
            ));

        for tc in tool_calls {
            self.post_event(
                "tool_use",
                serde_json::json!({
                    "tool_name": tc.name,
                    "input": truncate_for_preview(&tc.arguments.to_string(), 500),
                }),
            )
            .await;

            let job_ctx = JobContext {
                extra_env: self.extra_env.clone(),
                ..Default::default()
            };

            let result =
                execute_tool_simple(&self.tools, &self.safety, &tc.name, &tc.arguments, &job_ctx)
                    .await;
            let (tool_result_content, message) =
                process_tool_result(&self.safety, &tc.name, &tc.id, &result);

            self.post_event(
                "tool_result",
                serde_json::json!({
                    "tool_name": tc.name,
                    "output": truncate_for_preview(&tool_result_content, 2000),
                    "success": result.is_ok(),
                }),
            )
            .await;

            if let Ok(ref output) = result {
                *self.last_output.lock().await = output.clone();
            }

            reason_ctx.messages.push(message);
        }

        Ok(None)
    }

    async fn on_tool_intent_nudge(&self, text: &str, _reason_ctx: &mut ReasoningContext) {
        self.post_event(
            "message",
            serde_json::json!({
                "role": "assistant",
                "content": truncate_for_preview(text, 2000),
                "nudge": true,
            }),
        )
        .await;
    }

    async fn after_iteration(&self, _iteration: usize) {
        // Sleep for 100ms between iterations so the delegate does not busy-loop
        // while still yielding frequently enough for event delivery and other
        // runtime tasks to make progress.
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
