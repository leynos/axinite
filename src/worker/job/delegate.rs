//! `JobDelegate`: the `LoopDelegate` implementation for background jobs.
//!
//! Handles: signal channel (stop/ping/user messages), cancellation checks,
//! rate-limit retry, parallel tool execution, DB persistence, SSE broadcasting.

use std::time::Duration;

use tokio::sync::mpsc;

use crate::agent::agentic_loop::{
    LoopOutcome, LoopSignal, NativeLoopDelegate, TextAction, truncate_for_preview,
};
use crate::agent::scheduler::WorkerMessage;
use crate::context::JobState;
use crate::llm::{
    ChatMessage, Reasoning, ReasoningContext, RespondResult, ToolCall, ToolSelection,
};

use super::Worker;

/// Job delegate: implements `LoopDelegate` for the background job context.
///
/// Handles: signal channel (stop/ping/user messages), cancellation checks,
/// rate-limit retry, parallel tool execution, DB persistence, SSE broadcasting.
pub(super) struct JobDelegate<'a> {
    pub(super) worker: &'a Worker,
    pub(super) rx: tokio::sync::Mutex<&'a mut mpsc::Receiver<WorkerMessage>>,
    /// Tracks consecutive rate-limit errors to fail fast instead of burning iterations.
    pub(super) consecutive_rate_limits: std::sync::atomic::AtomicUsize,
}

impl<'a> JobDelegate<'a> {
    const MAX_CONSECUTIVE_RATE_LIMITS: usize = 10;

    /// Handle a rate-limit error: back off, increment counter, and fail fast
    /// if the provider remains rate-limited for too many consecutive attempts.
    async fn handle_rate_limit(
        &self,
        retry_after: Option<Duration>,
        context: &str,
    ) -> Result<crate::llm::RespondOutput, crate::error::Error> {
        use std::sync::atomic::Ordering::Relaxed;

        let count = self.consecutive_rate_limits.fetch_add(1, Relaxed) + 1;
        let wait = retry_after.unwrap_or(Duration::from_secs(5));
        tracing::warn!(
            job_id = %self.worker.job_id,
            wait_secs = wait.as_secs(),
            attempt = count,
            "LLM rate limited during {}, backing off",
            context,
        );

        if count >= Self::MAX_CONSECUTIVE_RATE_LIMITS {
            return Err(crate::error::JobError::Failed {
                id: self.worker.job_id,
                reason: "Persistent rate limiting: exceeded retry limit".to_string(),
            }
            .into());
        }

        self.worker.log_event(
            "status",
            serde_json::json!({
                "message": format!(
                    "Rate limited, retrying in {}s... ({}/{})",
                    wait.as_secs(), count, Self::MAX_CONSECUTIVE_RATE_LIMITS
                ),
            }),
        );
        tokio::time::sleep(wait).await;

        Ok(crate::llm::RespondOutput {
            result: RespondResult::Text(String::new()),
            usage: crate::llm::TokenUsage::default(),
        })
    }

    /// Reset the consecutive rate-limit counter after a successful LLM call.
    fn reset_rate_limit_counter(&self) {
        self.consecutive_rate_limits
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Attempt tool selection via `select_tools`.
    ///
    /// Returns `Ok(Some(output))` when the loop has a usable response (either
    /// tool calls or a rate-limit backoff placeholder), or `Ok(None)` when the
    /// selection was empty and the caller should fall back to
    /// `respond_with_tools`.
    async fn try_select_tools(
        &self,
        reasoning: &Reasoning,
        reason_ctx: &mut ReasoningContext,
    ) -> Result<Option<crate::llm::RespondOutput>, crate::error::Error> {
        match reasoning.select_tools(reason_ctx).await {
            Ok(s) if !s.is_empty() => {
                self.reset_rate_limit_counter();
                let tool_calls: Vec<ToolCall> = selections_to_tool_calls(&s);
                Ok(Some(crate::llm::RespondOutput {
                    result: RespondResult::ToolCalls {
                        tool_calls,
                        content: None,
                    },
                    usage: crate::llm::TokenUsage::default(),
                }))
            }
            Ok(_) => Ok(None), // empty selections, fall back
            Err(crate::error::LlmError::RateLimited { retry_after, .. }) => self
                .handle_rate_limit(retry_after, "tool selection")
                .await
                .map(Some),
            Err(e) => Err(e.into()),
        }
    }

    /// Charge the job's token budget for a completed `respond_with_tools` call.
    ///
    /// Fails the job when the budget update reports the limit as exceeded.
    ///
    /// NOTE: `select_tools()` also makes LLM calls but doesn't expose
    /// `TokenUsage`; only `respond_with_tools()` usage is tracked here.
    async fn track_token_budget(
        &self,
        usage: &crate::llm::TokenUsage,
    ) -> Result<(), crate::error::Error> {
        let total_tokens = usage.total() as u64;
        if total_tokens == 0 {
            return Ok(());
        }
        if let Err(msg) = self
            .worker
            .context_manager()
            .update_context(self.worker.job_id, |ctx| ctx.add_tokens(total_tokens))
            .await?
        {
            return Err(crate::error::JobError::Failed {
                id: self.worker.job_id,
                reason: msg,
            }
            .into());
        }
        Ok(())
    }

    /// Call `respond_with_tools`, tracking token usage and rate limits.
    async fn respond_with_tools_tracked(
        &self,
        reasoning: &Reasoning,
        reason_ctx: &mut ReasoningContext,
    ) -> Result<crate::llm::RespondOutput, crate::error::Error> {
        match reasoning.respond_with_tools(reason_ctx).await {
            Ok(output) => {
                self.reset_rate_limit_counter();
                self.track_token_budget(&output.usage).await?;
                Ok(output)
            }
            Err(crate::error::LlmError::RateLimited { retry_after, .. }) => {
                self.handle_rate_limit(retry_after, "respond_with_tools")
                    .await
            }
            Err(e) => Err(e.into()),
        }
    }
}

impl<'a> NativeLoopDelegate for JobDelegate<'a> {
    async fn check_signals(&self) -> LoopSignal {
        // Drain the entire message channel, prioritizing Stop over user messages.
        // Scope the lock so it's dropped before any .await below.
        let mut stop_requested = false;
        let mut first_user_message: Option<String> = None;
        {
            let mut rx = self.rx.lock().await;
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    WorkerMessage::Stop => {
                        tracing::debug!(
                            "Worker for job {} received stop signal",
                            self.worker.job_id
                        );
                        stop_requested = true;
                    }
                    WorkerMessage::Ping => {
                        tracing::trace!("Worker for job {} received ping", self.worker.job_id);
                    }
                    WorkerMessage::Start => {}
                    WorkerMessage::UserMessage(content) => {
                        tracing::info!(
                            job_id = %self.worker.job_id,
                            "Worker received follow-up user message"
                        );
                        self.worker.log_event(
                            "message",
                            serde_json::json!({
                                "role": "user",
                                "content": content,
                            }),
                        );
                        // Keep only the first user message; subsequent ones will be
                        // picked up on the next iteration's drain.
                        if first_user_message.is_none() {
                            first_user_message = Some(content);
                        }
                    }
                }
            }
        } // MutexGuard dropped here, before the cancellation .await

        // Stop takes priority over user messages
        if stop_requested {
            return LoopSignal::Stop;
        }

        if let Some(content) = first_user_message {
            return LoopSignal::InjectMessage(content);
        }

        // Check for terminal or non-progressing state. The loop should stop when the
        // job has been cancelled, failed, stuck, or already completed — not just the
        // three states that `is_terminal()` covers (Accepted/Failed/Cancelled).
        if let Ok(ctx) = self
            .worker
            .context_manager()
            .get_context(self.worker.job_id)
            .await
            && matches!(
                ctx.state,
                JobState::Cancelled
                    | JobState::Failed
                    | JobState::Stuck
                    | JobState::Completed
                    | JobState::Submitted
                    | JobState::Accepted
            )
        {
            tracing::info!(
                "Worker for job {} detected terminal state {:?}",
                self.worker.job_id,
                ctx.state,
            );
            return LoopSignal::Stop;
        }

        LoopSignal::Continue
    }

    async fn before_llm_call(
        &self,
        reason_ctx: &mut ReasoningContext,
        _iteration: usize,
    ) -> Option<LoopOutcome> {
        // Refresh tool definitions so newly built tools become visible
        reason_ctx.available_tools = self.worker.tools().tool_definitions().await;
        None
    }

    async fn call_llm(
        &self,
        reasoning: &Reasoning,
        reason_ctx: &mut ReasoningContext,
        _iteration: usize,
    ) -> Result<crate::llm::RespondOutput, crate::error::Error> {
        // Try select_tools first, fall back to respond_with_tools
        if let Some(output) = self.try_select_tools(reasoning, reason_ctx).await? {
            return Ok(output);
        }
        self.respond_with_tools_tracked(reasoning, reason_ctx).await
    }

    async fn handle_text_response(
        &self,
        text: &str,
        reason_ctx: &mut ReasoningContext,
    ) -> TextAction {
        // Empty text from rate-limit backoff retry — skip processing and let the
        // loop proceed to the next iteration which will re-call the LLM.
        if text.is_empty() {
            return TextAction::Continue;
        }

        // Check for explicit completion
        if crate::util::llm_signals_completion(text) {
            return TextAction::Return(LoopOutcome::Response(text.to_string()));
        }

        // Add assistant response to context
        reason_ctx.messages.push(ChatMessage::assistant(text));

        self.worker.log_event(
            "message",
            serde_json::json!({
                "role": "assistant",
                "content": text,
            }),
        );

        TextAction::Continue
    }

    async fn execute_tool_calls(
        &self,
        tool_calls: Vec<crate::llm::ToolCall>,
        content: Option<String>,
        reason_ctx: &mut ReasoningContext,
    ) -> Result<Option<LoopOutcome>, crate::error::Error> {
        if let Some(ref text) = content {
            self.worker.log_event(
                "message",
                serde_json::json!({
                    "role": "assistant",
                    "content": text,
                }),
            );
        }

        // Add assistant message with tool_calls (OpenAI protocol)
        reason_ctx
            .messages
            .push(ChatMessage::assistant_with_tool_calls(
                content,
                tool_calls.clone(),
            ));

        // Convert to ToolSelections
        let selections: Vec<ToolSelection> = tool_calls
            .iter()
            .map(|tc| ToolSelection {
                tool_name: tc.name.clone(),
                parameters: tc.arguments.clone(),
                reasoning: String::new(),
                alternatives: vec![],
                tool_call_id: tc.id.clone(),
            })
            .collect();

        // Execute tools (parallel for multiple, direct for single)
        if selections.len() == 1 {
            let selection = &selections[0];
            let result = self
                .worker
                .execute_tool(&selection.tool_name, &selection.parameters)
                .await;
            self.worker
                .process_tool_result_job(reason_ctx, selection, result)
                .await?;
        } else {
            let results = self.worker.execute_tools_parallel(&selections).await;
            for (selection, result) in selections.iter().zip(results) {
                self.worker
                    .process_tool_result_job(reason_ctx, selection, result.result)
                    .await?;
            }
        }

        Ok(None)
    }

    async fn on_tool_intent_nudge(&self, text: &str, _reason_ctx: &mut ReasoningContext) {
        self.worker.log_event(
            "message",
            serde_json::json!({
                "role": "assistant",
                "content": truncate_for_preview(text, 2000),
                "nudge": true,
            }),
        );
    }

    async fn after_iteration(&self, _iteration: usize) {
        // Small delay between iterations
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Convert `ToolSelection`s to `ToolCall`s.
fn selections_to_tool_calls(selections: &[ToolSelection]) -> Vec<ToolCall> {
    selections
        .iter()
        .map(|s| ToolCall {
            id: s.tool_call_id.clone(),
            name: s.tool_name.clone(),
            arguments: s.parameters.clone(),
        })
        .collect()
}
