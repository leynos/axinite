//! Recording helpers for chat tool execution.
//!
//! Persists redacted tool calls and writes indexed outcomes back to the
//! current turn so later results stay aligned with the originating call.

use crate::agent::dispatcher::delegate::ChatDelegate;

/// Compute the safe (redacted) argument map for a single tool call.
async fn redact_single_tool_call(
    agent: &crate::agent::Agent,
    tc: &crate::llm::ToolCall,
) -> serde_json::Value {
    if let Some(tool) = agent.tools().get(&tc.name).await {
        crate::tools::redact_params(&tc.arguments, tool.sensitive_params())
    } else {
        tc.arguments.clone()
    }
}

/// Record redacted tool-call args into the current turn of the session thread.
pub(super) async fn write_tool_calls_to_thread(
    delegate: &ChatDelegate<'_>,
    tool_calls: &[crate::llm::ToolCall],
    redacted_args: Vec<serde_json::Value>,
) {
    let mut sess = delegate.session.lock().await;
    let Some(thread) = sess.threads.get_mut(&delegate.thread_id) else {
        return;
    };
    let Some(turn) = thread.last_turn_mut() else {
        return;
    };
    for (tc, safe_args) in tool_calls.iter().zip(redacted_args) {
        turn.record_tool_call(&tc.name, safe_args);
    }
}

/// Record tool calls in the session thread with sensitive params redacted.
pub(super) async fn record_redacted_tool_calls(
    delegate: &ChatDelegate<'_>,
    tool_calls: &[crate::llm::ToolCall],
) {
    let mut redacted_args: Vec<serde_json::Value> = Vec::with_capacity(tool_calls.len());
    for tc in tool_calls {
        redacted_args.push(redact_single_tool_call(delegate.agent, tc).await);
    }
    write_tool_calls_to_thread(delegate, tool_calls, redacted_args).await;
}

/// Record tool outcome in the thread.
pub(super) async fn record_tool_outcome(
    delegate: &ChatDelegate<'_>,
    tool_call_idx: usize,
    result_content: &str,
    is_tool_error: bool,
) {
    let mut sess = delegate.session.lock().await;
    if let Some(thread) = sess.threads.get_mut(&delegate.thread_id)
        && let Some(turn) = thread.last_turn_mut()
    {
        let record_result = if is_tool_error {
            turn.record_tool_error_at(tool_call_idx, result_content.to_string())
        } else {
            turn.record_tool_result_content_at(tool_call_idx, result_content)
        };
        if let Err(error) = record_result {
            tracing::warn!(
                tool_call_idx,
                %error,
                "Failed to record tool outcome in session turn"
            );
        }
    }
}
