//! Unified agentic loop engine.
//!
//! Provides a single implementation of the core LLM call → tool execution →
//! result processing → context update → repeat cycle. Three consumers
//! (chat dispatcher, job worker, container runtime) customize behaviour
//! via the `LoopDelegate` trait.

use core::future::Future;
use core::pin::Pin;

use crate::agent::session::PendingApproval;
use crate::error::Error;
use crate::llm::{ChatMessage, Reasoning, ReasoningContext, RespondResult};

/// Signal from the delegate indicating how the loop should proceed.
pub enum LoopSignal {
    /// Continue normally.
    Continue,
    /// Stop the loop gracefully.
    Stop,
    /// Inject a user message into context and continue.
    InjectMessage(String),
}

/// Outcome of a text response from the LLM.
pub enum TextAction {
    /// Return this as the final loop result.
    Return(LoopOutcome),
    /// Continue the loop (text was handled but loop should proceed).
    Continue,
}

/// Final outcome of the agentic loop.
pub enum LoopOutcome {
    /// Completed with a text response.
    Response(String),
    /// Loop was stopped by a signal.
    Stopped,
    /// Max iterations exceeded.
    MaxIterations,
    /// A tool requires user approval before continuing (chat delegate only).
    NeedApproval(Box<PendingApproval>),
}

/// Configuration for the agentic loop.
pub struct AgenticLoopConfig {
    pub max_iterations: usize,
    pub enable_tool_intent_nudge: bool,
    pub max_tool_intent_nudges: u32,
}

impl Default for AgenticLoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: 50,
            enable_tool_intent_nudge: true,
            max_tool_intent_nudges: 2,
        }
    }
}

/// Boxed future used by the dyn-facing agentic-loop boundary.
pub type LoopDelegateFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

macro_rules! delegate_methods {
    (dyn_safe) => {
        /// Called at the start of each iteration. Check for external signals
        /// (cancellation, user messages, stop requests).
        fn check_signals(&self) -> LoopDelegateFuture<'_, LoopSignal>;

        /// Called before the LLM call. Allows the delegate to refresh tool
        /// definitions, enforce cost guards, or inject messages.
        /// Return `Some(outcome)` to break the loop early.
        fn before_llm_call<'a>(
            &'a self,
            reason_ctx: &'a mut ReasoningContext,
            iteration: usize,
        ) -> LoopDelegateFuture<'a, Option<LoopOutcome>>;

        /// Call the LLM and return the result. Delegates own the LLM call
        /// to handle consumer-specific concerns (rate limiting, auto-compaction,
        /// cost tracking, force_text mode).
        fn call_llm<'a>(
            &'a self,
            reasoning: &'a Reasoning,
            reason_ctx: &'a mut ReasoningContext,
            iteration: usize,
        ) -> LoopDelegateFuture<'a, Result<crate::llm::RespondOutput, Error>>;

        /// Handle a text-only response from the LLM.
        /// Return `TextAction::Return` to exit the loop, `TextAction::Continue` to proceed.
        fn handle_text_response<'a>(
            &'a self,
            text: &'a str,
            reason_ctx: &'a mut ReasoningContext,
        ) -> LoopDelegateFuture<'a, TextAction>;

        /// Execute tool calls and add results to context.
        /// Return `Some(outcome)` to break the loop (e.g. approval needed).
        fn execute_tool_calls<'a>(
            &'a self,
            tool_calls: Vec<crate::llm::ToolCall>,
            content: Option<String>,
            reason_ctx: &'a mut ReasoningContext,
        ) -> LoopDelegateFuture<'a, Result<Option<LoopOutcome>, Error>>;

        /// Called when the LLM expresses tool intent without actually calling a tool.
        /// Delegates can use this to emit events or log the nudge for observability.
        fn on_tool_intent_nudge<'a>(
            &'a self,
            _text: &'a str,
            _reason_ctx: &'a mut ReasoningContext,
        ) -> LoopDelegateFuture<'a, ()> {
            Box::pin(async {})
        }

        /// Called after each successful iteration (no error, no early return).
        fn after_iteration(&self, _iteration: usize) -> LoopDelegateFuture<'_, ()> {
            Box::pin(async {})
        }
    };
    (native) => {
        /// See [`LoopDelegate::check_signals`].
        fn check_signals(&self) -> impl Future<Output = LoopSignal> + Send + '_;

        /// See [`LoopDelegate::before_llm_call`].
        fn before_llm_call<'a>(
            &'a self,
            reason_ctx: &'a mut ReasoningContext,
            iteration: usize,
        ) -> impl Future<Output = Option<LoopOutcome>> + Send + 'a;

        /// See [`LoopDelegate::call_llm`].
        fn call_llm<'a>(
            &'a self,
            reasoning: &'a Reasoning,
            reason_ctx: &'a mut ReasoningContext,
            iteration: usize,
        ) -> impl Future<Output = Result<crate::llm::RespondOutput, Error>> + Send + 'a;

        /// See [`LoopDelegate::handle_text_response`].
        fn handle_text_response<'a>(
            &'a self,
            text: &'a str,
            reason_ctx: &'a mut ReasoningContext,
        ) -> impl Future<Output = TextAction> + Send + 'a;

        /// See [`LoopDelegate::execute_tool_calls`].
        fn execute_tool_calls<'a>(
            &'a self,
            tool_calls: Vec<crate::llm::ToolCall>,
            content: Option<String>,
            reason_ctx: &'a mut ReasoningContext,
        ) -> impl Future<Output = Result<Option<LoopOutcome>, Error>> + Send + 'a;

        /// See [`LoopDelegate::on_tool_intent_nudge`].
        fn on_tool_intent_nudge<'a>(
            &'a self,
            _text: &'a str,
            _reason_ctx: &'a mut ReasoningContext,
        ) -> impl Future<Output = ()> + Send + 'a {
            async {}
        }

        /// See [`LoopDelegate::after_iteration`].
        fn after_iteration(&self, _iteration: usize) -> impl Future<Output = ()> + Send + '_ {
            async {}
        }
    };
}

/// Strategy trait — each consumer implements this to customize I/O and lifecycle.
///
/// The shared loop calls these methods at well-defined points. Consumers
/// implement only the behaviour that differs between chat, job, and container
/// contexts. The loop itself handles the common logic: tool intent nudge,
/// iteration counting, tool definition refresh, and the respond → execute → process cycle.
pub trait LoopDelegate: Send + Sync {
    delegate_methods!(dyn_safe);
}

/// Native async sibling trait for concrete agentic-loop implementations.
pub trait NativeLoopDelegate: Send + Sync {
    delegate_methods!(native);
}

impl<T> LoopDelegate for T
where
    T: NativeLoopDelegate + Send + Sync,
{
    fn check_signals(&self) -> LoopDelegateFuture<'_, LoopSignal> {
        Box::pin(NativeLoopDelegate::check_signals(self))
    }

    fn before_llm_call<'a>(
        &'a self,
        reason_ctx: &'a mut ReasoningContext,
        iteration: usize,
    ) -> LoopDelegateFuture<'a, Option<LoopOutcome>> {
        Box::pin(
            async move { NativeLoopDelegate::before_llm_call(self, reason_ctx, iteration).await },
        )
    }

    fn call_llm<'a>(
        &'a self,
        reasoning: &'a Reasoning,
        reason_ctx: &'a mut ReasoningContext,
        iteration: usize,
    ) -> LoopDelegateFuture<'a, Result<crate::llm::RespondOutput, Error>> {
        Box::pin(async move {
            NativeLoopDelegate::call_llm(self, reasoning, reason_ctx, iteration).await
        })
    }

    fn handle_text_response<'a>(
        &'a self,
        text: &'a str,
        reason_ctx: &'a mut ReasoningContext,
    ) -> LoopDelegateFuture<'a, TextAction> {
        Box::pin(
            async move { NativeLoopDelegate::handle_text_response(self, text, reason_ctx).await },
        )
    }

    fn execute_tool_calls<'a>(
        &'a self,
        tool_calls: Vec<crate::llm::ToolCall>,
        content: Option<String>,
        reason_ctx: &'a mut ReasoningContext,
    ) -> LoopDelegateFuture<'a, Result<Option<LoopOutcome>, Error>> {
        Box::pin(async move {
            NativeLoopDelegate::execute_tool_calls(self, tool_calls, content, reason_ctx).await
        })
    }

    fn on_tool_intent_nudge<'a>(
        &'a self,
        text: &'a str,
        reason_ctx: &'a mut ReasoningContext,
    ) -> LoopDelegateFuture<'a, ()> {
        Box::pin(
            async move { NativeLoopDelegate::on_tool_intent_nudge(self, text, reason_ctx).await },
        )
    }

    fn after_iteration(&self, iteration: usize) -> LoopDelegateFuture<'_, ()> {
        Box::pin(async move { NativeLoopDelegate::after_iteration(self, iteration).await })
    }
}

/// Determine whether a text-only response that signals tool intent should
/// trigger a nudge instead of being accepted as the final answer.
fn should_nudge_tool_intent(
    config: &AgenticLoopConfig,
    reason_ctx: &ReasoningContext,
    nudges_so_far: u32,
    text: &str,
) -> bool {
    let nudging_enabled =
        config.enable_tool_intent_nudge && nudges_so_far < config.max_tool_intent_nudges;
    let tools_expected = !reason_ctx.available_tools.is_empty() && !reason_ctx.force_text;
    let nudge_applicable = nudging_enabled && tools_expected;
    nudge_applicable && crate::llm::llm_signals_tool_intent(text)
}

/// Run the unified agentic loop.
///
/// This is the single implementation used by all three consumers (chat, job, container).
/// The `delegate` provides consumer-specific behaviour via the `LoopDelegate` trait.
pub async fn run_agentic_loop(
    delegate: &dyn LoopDelegate,
    reasoning: &Reasoning,
    reason_ctx: &mut ReasoningContext,
    config: &AgenticLoopConfig,
) -> Result<LoopOutcome, Error> {
    let mut consecutive_tool_intent_nudges: u32 = 0;

    for iteration in 1..=config.max_iterations {
        // Check for external signals (stop, cancellation, user messages)
        match delegate.check_signals().await {
            LoopSignal::Continue => {}
            LoopSignal::Stop => return Ok(LoopOutcome::Stopped),
            LoopSignal::InjectMessage(msg) => {
                reason_ctx.messages.push(ChatMessage::user(&msg));
            }
        }

        // Pre-LLM call hook (cost guard, tool refresh, iteration limit nudge)
        if let Some(outcome) = delegate.before_llm_call(reason_ctx, iteration).await {
            return Ok(outcome);
        }

        // Call LLM
        let output = delegate.call_llm(reasoning, reason_ctx, iteration).await?;

        match output.result {
            RespondResult::Text(text) => {
                // Tool intent nudge: if the LLM says "let me search..." without
                // actually calling a tool, inject a nudge message.
                if should_nudge_tool_intent(
                    config,
                    reason_ctx,
                    consecutive_tool_intent_nudges,
                    &text,
                ) {
                    consecutive_tool_intent_nudges += 1;
                    tracing::info!(
                        iteration,
                        "LLM expressed tool intent without calling a tool, nudging"
                    );
                    delegate.on_tool_intent_nudge(&text, reason_ctx).await;
                    reason_ctx.messages.push(ChatMessage::assistant(&text));
                    reason_ctx
                        .messages
                        .push(ChatMessage::user(crate::llm::TOOL_INTENT_NUDGE));
                    delegate.after_iteration(iteration).await;
                    continue;
                }

                // Reset nudge counter since we got a non-intent text response
                if !crate::llm::llm_signals_tool_intent(&text) {
                    consecutive_tool_intent_nudges = 0;
                }

                match delegate.handle_text_response(&text, reason_ctx).await {
                    TextAction::Return(outcome) => return Ok(outcome),
                    TextAction::Continue => {}
                }
            }
            RespondResult::ToolCalls {
                tool_calls,
                content,
            } => {
                consecutive_tool_intent_nudges = 0;

                if let Some(outcome) = delegate
                    .execute_tool_calls(tool_calls, content, reason_ctx)
                    .await?
                {
                    return Ok(outcome);
                }
            }
        }

        delegate.after_iteration(iteration).await;
    }

    Ok(LoopOutcome::MaxIterations)
}

/// Truncate a string for log/status previews.
///
/// `max` is a byte budget. The result is truncated at the last valid char
/// boundary at or before `max` bytes, so it is always valid UTF-8.
pub fn truncate_for_preview(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let end = crate::util::floor_char_boundary(s, max);
        format!("{}...", &s[..end])
    }
}

#[cfg(test)]
mod tests;
