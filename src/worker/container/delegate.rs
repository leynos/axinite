//! Loop-delegate implementation for worker containers.
//!
//! This module bridges the agentic loop to the worker HTTP API, posting events,
//! polling follow-up prompts, executing tools, and pacing iterations so the
//! runtime remains responsive.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, mpsc};

use super::{WorkerHttpClient, available_tool_definitions};
use crate::agent::agentic_loop::{
    LoopOutcome, LoopSignal, NativeLoopDelegate, TextAction, truncate_for_preview,
};
use crate::context::JobContext;
use crate::llm::{ChatMessage, Reasoning, ReasoningContext};
use crate::safety::SafetyLayer;
use crate::tools::ToolRegistry;
use crate::tools::execute::{execute_tool_simple, process_tool_result};
use crate::worker::api::{JobEventPayload, JobEventType, StatusUpdate, WorkerState};

/// Capacity for the event channel; bounds memory growth if the orchestrator
/// becomes slow or unresponsive.
const EVENT_CHANNEL_CAPACITY: usize = 256;

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
    /// Sender for fire-and-forget event posting to the background worker.
    pub(super) event_sender: mpsc::Sender<JobEventPayload>,
    /// Handle for the background event-posting task.
    event_handle: tokio::task::JoinHandle<()>,
}

impl ContainerDelegate {
    /// Create a new [`ContainerDelegate`] with a background event-sender task.
    pub(super) fn new(
        client: Arc<WorkerHttpClient>,
        safety: Arc<SafetyLayer>,
        tools: Arc<ToolRegistry>,
        extra_env: Arc<HashMap<String, String>>,
        iteration_tracker: Arc<Mutex<u32>>,
    ) -> Self {
        let (event_sender, mut event_receiver) =
            mpsc::channel::<JobEventPayload>(EVENT_CHANNEL_CAPACITY);

        // Spawn background task to handle event POSTs asynchronously
        let bg_client = Arc::clone(&client);
        let event_handle = tokio::spawn(async move {
            while let Some(payload) = event_receiver.recv().await {
                if let Err(e) = bg_client.post_event(&payload).await {
                    tracing::warn!(error = %e, "Failed to post event");
                }
            }
        });

        Self {
            client,
            safety,
            tools,
            extra_env,
            last_output: Mutex::new(String::new()),
            iteration_tracker,
            event_sender,
            event_handle,
        }
    }

    /// Shut down the delegate, draining any buffered events.
    ///
    /// Closes the event channel and awaits the background worker so
    /// in-flight events are flushed before the delegate is dropped.
    pub(super) async fn shutdown(self) {
        drop(self.event_sender);
        if let Err(e) = self.event_handle.await {
            tracing::warn!(error = %e, "Event worker task panicked");
        }
    }

    pub(super) fn post_event(&self, event_type: JobEventType, data: serde_json::Value) {
        let payload = JobEventPayload { event_type, data };
        if let Err(e) = self.event_sender.try_send(payload) {
            tracing::warn!(error = %e, "Failed to enqueue event for posting");
        }
    }

    async fn poll_and_inject_prompt(&self, reason_ctx: &mut ReasoningContext) {
        match self.client.poll_prompt().await {
            Ok(Some(prompt)) => {
                tracing::info!(
                    "Received follow-up prompt: {}",
                    truncate_for_preview(&prompt.content, 100)
                );
                self.post_event(
                    JobEventType::Message,
                    serde_json::json!({
                        "role": "user",
                        "content": truncate_for_preview(&prompt.content, 2000),
                    }),
                );
                reason_ctx.messages.push(ChatMessage::user(&prompt.content));
            }
            Ok(None) => {}
            Err(e) => {
                tracing::debug!("Failed to poll for prompt: {}", e);
            }
        }
    }
}

impl NativeLoopDelegate for ContainerDelegate {
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
            self.client
                .report_status_lossy(&StatusUpdate::new(
                    WorkerState::InProgress,
                    Some(format!("Iteration {}", iteration)),
                    iteration,
                ))
                .await;
        }

        self.poll_and_inject_prompt(reason_ctx).await;
        reason_ctx.available_tools = available_tool_definitions(&self.tools).await;

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
            JobEventType::Message,
            serde_json::json!({
                "role": "assistant",
                "content": truncate_for_preview(text, 2000),
            }),
        );

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
                JobEventType::Message,
                serde_json::json!({
                    "role": "assistant",
                    "content": truncate_for_preview(text, 2000),
                }),
            );
        }

        reason_ctx
            .messages
            .push(ChatMessage::assistant_with_tool_calls(
                content,
                tool_calls.clone(),
            ));

        for tc in tool_calls {
            self.post_event(
                JobEventType::ToolUse,
                serde_json::json!({
                    "tool_name": tc.name,
                    "input": truncate_for_preview(&tc.arguments.to_string(), 500),
                }),
            );

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
                JobEventType::ToolResult,
                serde_json::json!({
                    "tool_name": tc.name,
                    "output": truncate_for_preview(&tool_result_content, 2000),
                    "success": result.is_ok(),
                }),
            );

            if let Ok(ref output) = result {
                *self.last_output.lock().await = output.clone();
            }

            reason_ctx.messages.push(message);
        }

        Ok(None)
    }

    async fn on_tool_intent_nudge(&self, text: &str, _reason_ctx: &mut ReasoningContext) {
        self.post_event(
            JobEventType::Message,
            serde_json::json!({
                "role": "assistant",
                "content": truncate_for_preview(text, 2000),
                "nudge": true,
            }),
        );
    }

    async fn after_iteration(&self, _iteration: usize) {
        // Sleep for 100ms between iterations so the delegate does not busy-loop
        // while still yielding frequently enough for event delivery and other
        // runtime tasks to make progress.
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
