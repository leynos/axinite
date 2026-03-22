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

/// Strategy trait — each consumer implements this to customize I/O and lifecycle.
///
/// The shared loop calls these methods at well-defined points. Consumers
/// implement only the behavior that differs between chat, job, and container
/// contexts. The loop itself handles the common logic: tool intent nudge,
/// iteration counting, tool definition refresh, and the respond → execute → process cycle.
pub trait LoopDelegate: Send + Sync {
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
}

/// Native async sibling trait for concrete agentic-loop implementations.
pub trait NativeLoopDelegate: Send + Sync {
    /// Called at the start of each iteration. Check for external signals
    /// (cancellation, user messages, stop requests).
    fn check_signals(&self) -> impl Future<Output = LoopSignal> + Send + '_;

    /// Called before the LLM call. Allows the delegate to refresh tool
    /// definitions, enforce cost guards, or inject messages.
    /// Return `Some(outcome)` to break the loop early.
    fn before_llm_call<'a>(
        &'a self,
        reason_ctx: &'a mut ReasoningContext,
        iteration: usize,
    ) -> impl Future<Output = Option<LoopOutcome>> + Send + 'a;

    /// Call the LLM and return the result. Delegates own the LLM call
    /// to handle consumer-specific concerns (rate limiting, auto-compaction,
    /// cost tracking, force_text mode).
    fn call_llm<'a>(
        &'a self,
        reasoning: &'a Reasoning,
        reason_ctx: &'a mut ReasoningContext,
        iteration: usize,
    ) -> impl Future<Output = Result<crate::llm::RespondOutput, Error>> + Send + 'a;

    /// Handle a text-only response from the LLM.
    /// Return `TextAction::Return` to exit the loop, `TextAction::Continue` to proceed.
    fn handle_text_response<'a>(
        &'a self,
        text: &'a str,
        reason_ctx: &'a mut ReasoningContext,
    ) -> impl Future<Output = TextAction> + Send + 'a;

    /// Execute tool calls and add results to context.
    /// Return `Some(outcome)` to break the loop (e.g. approval needed).
    fn execute_tool_calls<'a>(
        &'a self,
        tool_calls: Vec<crate::llm::ToolCall>,
        content: Option<String>,
        reason_ctx: &'a mut ReasoningContext,
    ) -> impl Future<Output = Result<Option<LoopOutcome>, Error>> + Send + 'a;

    /// Called when the LLM expresses tool intent without actually calling a tool.
    /// Delegates can use this to emit events or log the nudge for observability.
    fn on_tool_intent_nudge<'a>(
        &'a self,
        _text: &'a str,
        _reason_ctx: &'a mut ReasoningContext,
    ) -> impl Future<Output = ()> + Send + 'a {
        async {}
    }

    /// Called after each successful iteration (no error, no early return).
    fn after_iteration(&self, _iteration: usize) -> impl Future<Output = ()> + Send + '_ {
        async {}
    }
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

/// Run the unified agentic loop.
///
/// This is the single implementation used by all three consumers (chat, job, container).
/// The `delegate` provides consumer-specific behavior via the `LoopDelegate` trait.
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
                if config.enable_tool_intent_nudge
                    && !reason_ctx.available_tools.is_empty()
                    && !reason_ctx.force_text
                    && consecutive_tool_intent_nudges < config.max_tool_intent_nudges
                    && crate::llm::llm_signals_tool_intent(&text)
                {
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
mod tests {
    use super::*;
    use crate::llm::{RespondOutput, TokenUsage, ToolCall};
    use crate::testing::StubLlm;
    use rstest::rstest;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Mutex;

    fn stub_reasoning() -> Reasoning {
        Reasoning::new(Arc::new(StubLlm::default()))
    }

    fn zero_usage() -> TokenUsage {
        TokenUsage {
            input_tokens: 0,
            output_tokens: 0,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        }
    }

    fn text_output(text: &str) -> RespondOutput {
        RespondOutput {
            result: RespondResult::Text(text.to_string()),
            usage: zero_usage(),
        }
    }

    fn tool_calls_output(calls: Vec<ToolCall>) -> RespondOutput {
        RespondOutput {
            result: RespondResult::ToolCalls {
                tool_calls: calls,
                content: None,
            },
            usage: zero_usage(),
        }
    }

    /// Configurable mock delegate for testing run_agentic_loop.
    struct MockDelegate {
        signal: Mutex<LoopSignal>,
        llm_responses: Mutex<Vec<RespondOutput>>,
        tool_exec_count: AtomicUsize,
        tool_exec_outcome: Mutex<Option<LoopOutcome>>,
        iterations_seen: Mutex<Vec<usize>>,
        early_exit: Mutex<Option<(usize, LoopOutcome)>>,
        nudge_count: AtomicUsize,
    }

    impl MockDelegate {
        fn new(responses: Vec<RespondOutput>) -> Self {
            Self {
                signal: Mutex::new(LoopSignal::Continue),
                llm_responses: Mutex::new(responses),
                tool_exec_count: AtomicUsize::new(0),
                tool_exec_outcome: Mutex::new(None),
                iterations_seen: Mutex::new(Vec::new()),
                early_exit: Mutex::new(None),
                nudge_count: AtomicUsize::new(0),
            }
        }

        fn with_signal(mut self, signal: LoopSignal) -> Self {
            self.signal = Mutex::new(signal);
            self
        }

        fn with_early_exit(mut self, iteration: usize, outcome: LoopOutcome) -> Self {
            self.early_exit = Mutex::new(Some((iteration, outcome)));
            self
        }
    }

    impl NativeLoopDelegate for MockDelegate {
        async fn check_signals(&self) -> LoopSignal {
            let mut sig = self.signal.lock().await;
            std::mem::replace(&mut *sig, LoopSignal::Continue)
        }

        async fn before_llm_call(
            &self,
            _reason_ctx: &mut ReasoningContext,
            iteration: usize,
        ) -> Option<LoopOutcome> {
            let mut guard = self.early_exit.lock().await;
            let should_take = guard
                .as_ref()
                .is_some_and(|(target, _)| *target == iteration);
            if should_take {
                guard.take().map(|(_, o)| o)
            } else {
                None
            }
        }

        async fn call_llm(
            &self,
            _reasoning: &Reasoning,
            _reason_ctx: &mut ReasoningContext,
            _iteration: usize,
        ) -> Result<crate::llm::RespondOutput, crate::error::Error> {
            let mut responses = self.llm_responses.lock().await;
            if responses.is_empty() {
                panic!("MockDelegate: no more LLM responses queued");
            }
            Ok(responses.remove(0))
        }

        async fn handle_text_response(
            &self,
            text: &str,
            _reason_ctx: &mut ReasoningContext,
        ) -> TextAction {
            TextAction::Return(LoopOutcome::Response(text.to_string()))
        }

        async fn execute_tool_calls(
            &self,
            _tool_calls: Vec<ToolCall>,
            _content: Option<String>,
            reason_ctx: &mut ReasoningContext,
        ) -> Result<Option<LoopOutcome>, crate::error::Error> {
            self.tool_exec_count.fetch_add(1, Ordering::SeqCst);
            reason_ctx
                .messages
                .push(ChatMessage::user("tool result stub"));
            let outcome = self.tool_exec_outcome.lock().await.take();
            Ok(outcome)
        }

        async fn on_tool_intent_nudge(&self, _text: &str, _reason_ctx: &mut ReasoningContext) {
            self.nudge_count.fetch_add(1, Ordering::SeqCst);
        }

        async fn after_iteration(&self, iteration: usize) {
            self.iterations_seen.lock().await.push(iteration);
        }
    }

    // --- Tests ---

    #[tokio::test]
    async fn test_text_response_returns_immediately() {
        let delegate = MockDelegate::new(vec![text_output("Hello, world!")]);
        let reasoning = stub_reasoning();
        let mut ctx = ReasoningContext::new();
        let config = AgenticLoopConfig::default();

        let outcome = run_agentic_loop(&delegate, &reasoning, &mut ctx, &config)
            .await
            .unwrap();

        match outcome {
            LoopOutcome::Response(text) => assert_eq!(text, "Hello, world!"),
            _ => panic!("Expected LoopOutcome::Response"),
        }
        // after_iteration is NOT called when handle_text_response returns Return
        // (the loop exits before reaching after_iteration).
        assert!(delegate.iterations_seen.lock().await.is_empty());
    }

    #[tokio::test]
    async fn test_tool_call_then_text_response() {
        let tool_call = ToolCall {
            id: "call_1".to_string(),
            name: "echo".to_string(),
            arguments: serde_json::json!({}),
        };
        let delegate = MockDelegate::new(vec![
            tool_calls_output(vec![tool_call]),
            text_output("Done!"),
        ]);
        let reasoning = stub_reasoning();
        let mut ctx = ReasoningContext::new();
        let config = AgenticLoopConfig::default();

        let outcome = run_agentic_loop(&delegate, &reasoning, &mut ctx, &config)
            .await
            .unwrap();

        match outcome {
            LoopOutcome::Response(text) => assert_eq!(text, "Done!"),
            _ => panic!("Expected LoopOutcome::Response"),
        }
        assert_eq!(delegate.tool_exec_count.load(Ordering::SeqCst), 1);
        // after_iteration called for iteration 1 (tool call), but not 2
        // (text response exits before after_iteration).
        assert_eq!(*delegate.iterations_seen.lock().await, vec![1]);
    }

    #[tokio::test]
    async fn test_stop_signal_exits_immediately() {
        let delegate =
            MockDelegate::new(vec![text_output("unreachable")]).with_signal(LoopSignal::Stop);
        let reasoning = stub_reasoning();
        let mut ctx = ReasoningContext::new();
        let config = AgenticLoopConfig::default();

        let outcome = run_agentic_loop(&delegate, &reasoning, &mut ctx, &config)
            .await
            .unwrap();

        assert!(matches!(outcome, LoopOutcome::Stopped));
        assert!(delegate.iterations_seen.lock().await.is_empty());
    }

    #[tokio::test]
    async fn test_inject_message_adds_user_message() {
        let delegate = MockDelegate::new(vec![text_output("Got it")])
            .with_signal(LoopSignal::InjectMessage("injected prompt".to_string()));
        let reasoning = stub_reasoning();
        let mut ctx = ReasoningContext::new();
        let config = AgenticLoopConfig::default();

        let outcome = run_agentic_loop(&delegate, &reasoning, &mut ctx, &config)
            .await
            .unwrap();

        assert!(matches!(outcome, LoopOutcome::Response(_)));
        assert!(
            ctx.messages
                .iter()
                .any(|m| m.role == crate::llm::Role::User && m.content.contains("injected prompt")),
            "Injected message should appear in context"
        );
    }

    #[tokio::test]
    async fn test_max_iterations_reached() {
        struct ContinueDelegate;

        impl NativeLoopDelegate for ContinueDelegate {
            async fn check_signals(&self) -> LoopSignal {
                LoopSignal::Continue
            }
            async fn before_llm_call(
                &self,
                _: &mut ReasoningContext,
                _: usize,
            ) -> Option<LoopOutcome> {
                None
            }
            async fn call_llm(
                &self,
                _: &Reasoning,
                _: &mut ReasoningContext,
                _: usize,
            ) -> Result<crate::llm::RespondOutput, crate::error::Error> {
                Ok(text_output("still working"))
            }
            async fn handle_text_response(
                &self,
                _: &str,
                ctx: &mut ReasoningContext,
            ) -> TextAction {
                ctx.messages.push(ChatMessage::assistant("still working"));
                TextAction::Continue
            }
            async fn execute_tool_calls(
                &self,
                _: Vec<ToolCall>,
                _: Option<String>,
                _: &mut ReasoningContext,
            ) -> Result<Option<LoopOutcome>, crate::error::Error> {
                Ok(None)
            }
        }

        let delegate = ContinueDelegate;
        let reasoning = stub_reasoning();
        let mut ctx = ReasoningContext::new();
        let config = AgenticLoopConfig {
            max_iterations: 3,
            ..Default::default()
        };

        let outcome = run_agentic_loop(&delegate, &reasoning, &mut ctx, &config)
            .await
            .unwrap();

        assert!(matches!(outcome, LoopOutcome::MaxIterations));
        let assistant_count = ctx
            .messages
            .iter()
            .filter(|m| m.role == crate::llm::Role::Assistant)
            .count();
        assert_eq!(assistant_count, 3);
    }

    #[tokio::test]
    async fn test_tool_intent_nudge_fires_and_caps() {
        let delegate = MockDelegate::new(vec![
            text_output("Let me search for that file"),
            text_output("Let me search for that file"),
            text_output("Let me search for that file"),
        ]);
        let reasoning = stub_reasoning();
        let mut ctx = ReasoningContext::new();
        ctx.available_tools.push(crate::llm::ToolDefinition {
            name: "search".to_string(),
            description: "Search files".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        });
        let config = AgenticLoopConfig {
            max_iterations: 10,
            enable_tool_intent_nudge: true,
            max_tool_intent_nudges: 2,
        };

        let outcome = run_agentic_loop(&delegate, &reasoning, &mut ctx, &config)
            .await
            .unwrap();

        assert!(matches!(outcome, LoopOutcome::Response(_)));
        assert_eq!(delegate.nudge_count.load(Ordering::SeqCst), 2);
        let nudge_messages = ctx
            .messages
            .iter()
            .filter(|m| {
                m.role == crate::llm::Role::User
                    && m.content.contains("you did not include any tool calls")
            })
            .count();
        assert_eq!(
            nudge_messages, 2,
            "Should have exactly 2 nudge messages in context"
        );
    }

    #[tokio::test]
    async fn test_before_llm_call_early_exit() {
        let delegate = MockDelegate::new(vec![text_output("unreachable")])
            .with_early_exit(1, LoopOutcome::Stopped);
        let reasoning = stub_reasoning();
        let mut ctx = ReasoningContext::new();
        let config = AgenticLoopConfig::default();

        let outcome = run_agentic_loop(&delegate, &reasoning, &mut ctx, &config)
            .await
            .unwrap();

        assert!(matches!(outcome, LoopOutcome::Stopped));
        assert!(delegate.iterations_seen.lock().await.is_empty());
    }

    #[rstest]
    #[case("hello", 10, "hello")]
    #[case("hello", 5, "hello")]
    #[case("hello world", 5, "hello...")]
    #[case("é is fancy", 1, "...")]
    fn test_truncate_for_preview_cases(
        #[case] input: &str,
        #[case] max: usize,
        #[case] expected: &str,
    ) {
        assert_eq!(truncate_for_preview(input, max), expected);
    }
}
