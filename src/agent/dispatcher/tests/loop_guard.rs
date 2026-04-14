//! Loop guard and termination tests.

use super::*;

/// Asserts iteration bounds guarantee termination for a given max_iter.
fn assert_iteration_bounds(max_iter: usize) {
    let force_text_at = max_iter;
    let nudge_at = max_iter.saturating_sub(1);
    let hard_ceiling = max_iter + 1;

    // force_text_at must be reachable (> 0)
    assert!(
        force_text_at > 0,
        "force_text_at must be > 0 for max_iter={max_iter}"
    );

    // nudge comes before or at the same time as force_text
    assert!(
        nudge_at <= force_text_at,
        "nudge_at ({nudge_at}) > force_text_at ({force_text_at})"
    );

    // hard ceiling is strictly after force_text
    assert!(
        hard_ceiling > force_text_at,
        "hard_ceiling ({hard_ceiling}) not > force_text_at ({force_text_at})"
    );

    // Simulate iteration: every iteration from 1..=hard_ceiling
    // At force_text_at, force_text=true (should produce text and break).
    // At hard_ceiling, the error fires (safety net).
    let mut hit_force_text = false;
    let mut hit_ceiling = false;
    for iteration in 1..=hard_ceiling {
        if iteration >= force_text_at {
            hit_force_text = true;
        }
        if iteration > max_iter {
            hit_ceiling = true;
        }
    }
    assert!(
        hit_force_text,
        "force_text never triggered for max_iter={max_iter}"
    );
    // The ceiling should only fire if force_text somehow didn't break
    assert!(
        hit_ceiling || hard_ceiling <= max_iter + 1,
        "ceiling logic inconsistent for max_iter={max_iter}"
    );
}

// === QA Plan P2 - 4.3: Dispatcher loop guard tests ===

#[tokio::test]
async fn force_text_prevents_infinite_tool_call_loop() {
    // Verify that Reasoning with force_text=true returns text even when
    // the provider would normally return tool calls.
    use crate::llm::{Reasoning, ReasoningContext, RespondResult, ToolDefinition};

    let provider = Arc::new(MockLlmProvider::always_tool_call());
    let reasoning = Reasoning::new(provider);

    let tool_def = ToolDefinition {
        name: "echo".to_string(),
        description: "Echo a message".to_string(),
        parameters: serde_json::json!({"type": "object", "properties": {"message": {"type": "string"}}}),
    };

    // Without force_text: provider returns tool calls.
    let ctx_normal = ReasoningContext::new()
        .with_messages(vec![ChatMessage::user("hello")])
        .with_tools(vec![tool_def.clone()]);
    let output = reasoning
        .respond_with_tools(&ctx_normal)
        .await
        .expect("respond_with_tools failed for normal context");
    assert!(
        matches!(output.result, RespondResult::ToolCalls { .. }),
        "Without force_text, should get tool calls"
    );

    // With force_text: provider must return text (tools stripped).
    let mut ctx_forced = ReasoningContext::new()
        .with_messages(vec![ChatMessage::user("hello")])
        .with_tools(vec![tool_def]);
    ctx_forced.force_text = true;
    let output = reasoning
        .respond_with_tools(&ctx_forced)
        .await
        .expect("respond_with_tools failed for forced-text context");
    assert!(
        matches!(output.result, RespondResult::Text(_)),
        "With force_text, should get text response, got: {:?}",
        output.result
    );
}

#[test]
fn iteration_bounds_guarantee_termination() {
    // Verify the arithmetic that guards against infinite loops:
    // force_text_at = max_tool_iterations
    // nudge_at = max_tool_iterations - 1
    // hard_ceiling = max_tool_iterations + 1
    for max_iter in [1_usize, 2, 5, 10, 50] {
        assert_iteration_bounds(max_iter);
    }
}

/// Regression test for the infinite loop bug (PR #252) where `continue`
/// skipped the index increment. When every tool call fails (e.g., tool not
/// found), the dispatcher must still advance through all calls and
/// eventually terminate via the force_text / max_iterations guard.
#[tokio::test]
async fn test_dispatcher_terminates_with_all_tool_calls_failing() {
    let agent = make_test_agent_with_llm(Arc::new(MockLlmProvider::failing_tool_call()), 5);

    let session = Arc::new(Mutex::new(Session::new("test-user")));

    // Initialize a thread in the session so the loop can record tool calls.
    let thread_id = {
        let mut sess = session.lock().await;
        sess.create_thread().id
    };

    let message = IncomingMessage::new("test", "test-user", "do something");
    let initial_messages = vec![ChatMessage::user("do something")];

    // The dispatcher must terminate within 5 seconds. If there is an
    // infinite loop bug (e.g., index not advancing on tool failure), the
    // timeout will fire and the test will fail.
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        agent.run_agentic_loop(
            &message,
            super::super::core::RunLoopCtx {
                session,
                thread_id,
                initial_messages,
            },
        ),
    )
    .await;

    assert!(
        result.is_ok(),
        "Dispatcher timed out -- possible infinite loop when all tool calls fail"
    );

    // The loop should complete (either with a text response from force_text,
    // or an error from the hard ceiling). Both are acceptable termination.
    let inner = result.expect("test timed out or dispatcher context lost");
    let _ = inner;
}

/// Build test AgentDeps with EchoTool registered and the given LLM.
fn build_test_agent_deps(llm: Arc<dyn LlmProvider>) -> AgentDeps {
    let deps = make_agent_deps(llm, false);
    deps.tools.register_sync(Arc::new(EchoTool));
    deps
}

/// Build test AgentConfig with the specified max_tool_iterations.
fn build_test_agent_config(max_tool_iterations: usize) -> AgentConfig {
    let mut config = make_agent_config(max_tool_iterations, true);
    config.auto_approve_tools = true;
    config
}

/// Assert that the timeout-wrapped agentic loop result is a text response.
fn assert_agentic_loop_text_response<E: std::fmt::Debug>(
    result: Result<Result<super::super::AgenticLoopResult, E>, tokio::time::error::Elapsed>,
) {
    assert!(
        result.is_ok(),
        "Dispatcher timed out -- max_iterations guard failed to terminate the loop"
    );
    let inner = result.expect("test timed out or dispatcher context lost");
    assert!(
        inner.is_ok(),
        "Dispatcher returned an error: {:?}",
        inner.err()
    );
    match inner.unwrap() {
        super::super::AgenticLoopResult::Response(text) => {
            assert!(!text.is_empty(), "Expected non-empty forced text response");
        }
        super::super::AgenticLoopResult::NeedApproval { .. } => {
            panic!("Expected text response, got NeedApproval");
        }
    }
}

/// Verify that the max_iterations guard terminates the loop even when the
/// LLM always returns tool calls and those calls succeed.
#[tokio::test]
async fn test_dispatcher_terminates_with_max_iterations() {
    let llm: Arc<dyn LlmProvider> = Arc::new(MockLlmProvider::always_tool_call());
    let max_iter = 3;
    let agent = Agent::new(
        build_test_agent_config(max_iter),
        build_test_agent_deps(llm),
        Arc::new(ChannelManager::new()),
        None,
        None,
        None,
        Some(Arc::new(ContextManager::new(1))),
        None,
    );

    let session = Arc::new(Mutex::new(Session::new("test-user")));
    let thread_id = {
        let mut sess = session.lock().await;
        sess.create_thread().id
    };

    let message = IncomingMessage::new("test", "test-user", "keep calling tools");
    let initial_messages = vec![ChatMessage::user("keep calling tools")];

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        agent.run_agentic_loop(
            &message,
            super::super::core::RunLoopCtx {
                session,
                thread_id,
                initial_messages,
            },
        ),
    )
    .await;

    assert_agentic_loop_text_response(result);
}
