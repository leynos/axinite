//! Per-iteration steps of the agentic loop: signal handling and the handling
//! of the LLM's text and tool-call responses.

use super::{
    AgenticLoopConfig, LoopDelegate, LoopOutcome, LoopSignal, TextAction, should_nudge_tool_intent,
};
use crate::error::Error;
use crate::llm::{ChatMessage, ReasoningContext};

/// Check for external signals (stop, cancellation, user messages).
///
/// `Some(outcome)` ends the loop.
pub(super) async fn apply_loop_signal(
    delegate: &dyn LoopDelegate,
    reason_ctx: &mut ReasoningContext,
) -> Option<LoopOutcome> {
    match delegate.check_signals().await {
        LoopSignal::Continue => None,
        LoopSignal::Stop => Some(LoopOutcome::Stopped),
        LoopSignal::InjectMessage(msg) => {
            reason_ctx.messages.push(ChatMessage::user(&msg));
            None
        }
    }
}

/// Per-run state of the agentic loop: the delegate, its configuration and the
/// consecutive tool-intent nudge count.
pub(super) struct LoopState<'a> {
    pub(super) delegate: &'a dyn LoopDelegate,
    pub(super) config: &'a AgenticLoopConfig,
    pub(super) consecutive_tool_intent_nudges: u32,
}

impl LoopState<'_> {
    /// Handle a text-only LLM response: nudge on unfulfilled tool intent, else
    /// let the delegate decide. `Some(outcome)` ends the loop; `None` runs the
    /// iteration's `after_iteration` hook and carries on.
    pub(super) async fn on_text(
        &mut self,
        reason_ctx: &mut ReasoningContext,
        iteration: usize,
        text: &str,
    ) -> Option<LoopOutcome> {
        // Tool intent nudge: if the LLM says "let me search..." without
        // actually calling a tool, inject a nudge message.
        if should_nudge_tool_intent(
            self.config,
            reason_ctx,
            self.consecutive_tool_intent_nudges,
            text,
        ) {
            self.consecutive_tool_intent_nudges += 1;
            tracing::info!(
                iteration,
                "LLM expressed tool intent without calling a tool, nudging"
            );
            self.delegate.on_tool_intent_nudge(text, reason_ctx).await;
            reason_ctx.messages.push(ChatMessage::assistant(text));
            reason_ctx
                .messages
                .push(ChatMessage::user(crate::llm::TOOL_INTENT_NUDGE));
            return None;
        }

        // Reset nudge counter since we got a non-intent text response
        if !crate::llm::llm_signals_tool_intent(text) {
            self.consecutive_tool_intent_nudges = 0;
        }

        match self.delegate.handle_text_response(text, reason_ctx).await {
            TextAction::Return(outcome) => Some(outcome),
            TextAction::Continue => None,
        }
    }

    /// Execute the LLM's tool calls. `Some(outcome)` ends the loop.
    pub(super) async fn on_tool_calls(
        &mut self,
        reason_ctx: &mut ReasoningContext,
        tool_calls: Vec<crate::llm::ToolCall>,
        content: Option<String>,
    ) -> Result<Option<LoopOutcome>, Error> {
        self.consecutive_tool_intent_nudges = 0;
        self.delegate
            .execute_tool_calls(tool_calls, content, reason_ctx)
            .await
    }
}
