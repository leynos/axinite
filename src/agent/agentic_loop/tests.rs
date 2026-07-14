//! Unit tests for the agentic loop's turn handling and tool dispatch.

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
        async fn before_llm_call(&self, _: &mut ReasoningContext, _: usize) -> Option<LoopOutcome> {
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
        async fn handle_text_response(&self, _: &str, ctx: &mut ReasoningContext) -> TextAction {
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
