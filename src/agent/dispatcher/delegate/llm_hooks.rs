//! LLM hook implementations for the chat delegate.
//!
//! Contains the LLM call hooks (check_signals, before_llm_call, call_llm,
//! handle_text_response) and helper functions for message compaction and
//! response sanitization.

use crate::agent::agentic_loop::{LoopOutcome, LoopSignal, TextAction};
use crate::agent::cost_guard::CostGuard;
use crate::agent::dispatcher::delegate::ChatDelegate;
use crate::agent::session::ThreadState;
use crate::channels::StatusUpdate;
use crate::error::Error;
use crate::history::LlmCallRecord;
use crate::llm::{ChatMessage, Reasoning, ReasoningContext};

/// Check if the agent loop should stop due to external signals.
pub(crate) async fn check_signals(delegate: &ChatDelegate<'_>) -> LoopSignal {
    let sess = delegate.session.lock().await;
    if let Some(thread) = sess.threads.get(&delegate.thread_id)
        && thread.state == ThreadState::Interrupted
    {
        return LoopSignal::Stop;
    }
    LoopSignal::Continue
}

/// Prepare context before calling the LLM.
pub(crate) async fn before_llm_call(
    delegate: &ChatDelegate<'_>,
    reason_ctx: &mut ReasoningContext,
    iteration: usize,
) -> Option<LoopOutcome> {
    // Inject a nudge message when approaching the iteration limit so the
    // LLM is aware it should produce a final answer on the next turn.
    if iteration == delegate.nudge_at {
        reason_ctx.messages.push(ChatMessage::system(
            "You are approaching the tool call limit. \
                 Provide your best final answer on the next response \
                 using the information you have gathered so far. \
                 Do not call any more tools.",
        ));
    }

    let force_text = iteration >= delegate.force_text_at;

    // Refresh tool definitions each iteration so newly built tools become visible
    let tool_defs = delegate.agent.tools().tool_definitions().await;

    // Apply trust-based tool attenuation if skills are active.
    let tool_defs = if !delegate.active_skills.is_empty() {
        let result = crate::skills::attenuate_tools(&tool_defs, &delegate.active_skills);
        tracing::debug!(
            min_trust = %result.min_trust,
            tools_available = result.tools.len(),
            tools_removed = result.removed_tools.len(),
            removed = ?result.removed_tools,
            explanation = %result.explanation,
            "Tool attenuation applied"
        );
        result.tools
    } else {
        tool_defs
    };

    // Update context for this iteration
    reason_ctx.available_tools = if force_text { Vec::new() } else { tool_defs };
    reason_ctx.system_prompt = if force_text {
        Some(delegate.cached_prompt_no_tools.clone())
    } else {
        None
    };
    reason_ctx.force_text = force_text;

    if force_text {
        tracing::info!(
            iteration,
            "Forcing text-only response (iteration limit reached)"
        );
    }

    let _ = delegate
        .agent
        .channels
        .send_status(
            &delegate.message.channel,
            StatusUpdate::Thinking("Calling LLM...".into()),
            &delegate.message.metadata,
        )
        .await;

    None
}

/// Call the LLM and handle context-length-exceeded errors.
pub(crate) async fn call_llm(
    delegate: &ChatDelegate<'_>,
    reasoning: &Reasoning,
    reason_ctx: &mut ReasoningContext,
    iteration: usize,
) -> Result<crate::llm::RespondOutput, Error> {
    check_cost_guardrail(delegate.agent.cost_guard()).await?;
    let output = invoke_with_retry(delegate, reasoning, reason_ctx, iteration).await?;
    record_and_log_cost(delegate, &output).await;
    Ok(output)
}

async fn check_cost_guardrail(cost_guard: &CostGuard) -> Result<(), Error> {
    if let Err(limit) = cost_guard.check_allowed().await {
        return Err(crate::error::LlmError::InvalidResponse {
            provider: "agent".to_string(),
            reason: limit.to_string(),
        }
        .into());
    }
    Ok(())
}

async fn invoke_with_retry(
    delegate: &ChatDelegate<'_>,
    reasoning: &Reasoning,
    reason_ctx: &mut ReasoningContext,
    iteration: usize,
) -> Result<crate::llm::RespondOutput, Error> {
    match reasoning.respond_with_tools(reason_ctx).await {
        Ok(output) => Ok(output),
        Err(crate::error::LlmError::ContextLengthExceeded { used, limit }) => {
            tracing::warn!(
                used,
                limit,
                iteration,
                "Context length exceeded, compacting messages and retrying"
            );
            record_partial_llm_call(delegate, u32::try_from(used).unwrap_or(u32::MAX)).await;
            reason_ctx.messages = compact_messages_for_retry(&reason_ctx.messages);
            if reason_ctx.force_text {
                reason_ctx.available_tools.clear();
            }
            check_cost_guardrail(delegate.agent.cost_guard()).await?;
            match reasoning.respond_with_tools(reason_ctx).await {
                Ok(output) => Ok(output),
                Err(retry_err) => {
                    if let crate::error::LlmError::ContextLengthExceeded {
                        used: retry_used, ..
                    } = retry_err
                    {
                        record_partial_llm_call(
                            delegate,
                            u32::try_from(retry_used).unwrap_or(u32::MAX),
                        )
                        .await;
                    }
                    tracing::error!(
                        original_used = used,
                        original_limit = limit,
                        retry_error = %retry_err,
                        "Retry after auto-compaction also failed"
                    );
                    Err(crate::error::Error::from(retry_err))
                }
            }
        }
        Err(e) => Err(e.into()),
    }
}

async fn record_and_log_cost(delegate: &ChatDelegate<'_>, output: &crate::llm::RespondOutput) {
    let model_name = delegate.agent.llm().active_model_name();
    let read_discount = delegate.agent.llm().cache_read_discount();
    let write_multiplier = delegate.agent.llm().cache_write_multiplier();
    let call_cost = delegate
        .agent
        .cost_guard()
        .record_llm_call(
            &model_name,
            output.usage.input_tokens,
            output.usage.output_tokens,
            output.usage.cache_read_input_tokens,
            output.usage.cache_creation_input_tokens,
            read_discount,
            write_multiplier,
            Some(delegate.agent.llm().cost_per_token()),
        )
        .await;
    tracing::debug!(
        "LLM call used {} input + {} output tokens (${:.6})",
        output.usage.input_tokens,
        output.usage.output_tokens,
        call_cost,
    );
}

async fn record_partial_llm_call(delegate: &ChatDelegate<'_>, used: u32) {
    let model_name = delegate.agent.llm().active_model_name();
    let read_discount = delegate.agent.llm().cache_read_discount();
    let write_multiplier = delegate.agent.llm().cache_write_multiplier();
    let call_cost = delegate
        .agent
        .cost_guard()
        .record_llm_call(
            &model_name,
            used,
            0,
            0,
            0,
            read_discount,
            write_multiplier,
            Some(delegate.agent.llm().cost_per_token()),
        )
        .await;

    let Some(store) = delegate.agent.store() else {
        return;
    };

    let purpose =
        "context_length_exceeded:auto_compaction_retry (partial/estimated input tokens only)";
    let record = LlmCallRecord {
        job_id: Some(delegate.job_ctx.job_id),
        conversation_id: delegate.job_ctx.conversation_id,
        provider: "agent",
        model: &model_name,
        input_tokens: used,
        output_tokens: 0,
        cost: call_cost,
        purpose: Some(purpose),
    };

    if let Err(error) = store.record_llm_call(&record).await {
        tracing::warn!(%error, "Failed to persist partial LLM call audit entry");
    }
}

/// Handle a text response from the LLM.
pub(crate) async fn handle_text_response(_delegate: &ChatDelegate<'_>, text: &str) -> TextAction {
    // Strip internal "[Called tool ...]" text that can leak when
    // provider flattening (e.g. NEAR AI) converts tool_calls to
    // plain text and the LLM echoes it back.
    let sanitized = strip_internal_tool_call_text(text);
    TextAction::Return(LoopOutcome::Response(sanitized))
}

/// Collect all System messages from the slice.
fn collect_system_messages(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    use crate::llm::Role;
    messages
        .iter()
        .filter(|m| m.role == Role::System)
        .cloned()
        .collect()
}

/// Compact messages when a User message is present.
fn compact_around_user_message(messages: &[ChatMessage], user_idx: usize) -> Vec<ChatMessage> {
    let mut compacted = collect_system_messages(&messages[..user_idx]);

    if user_idx > 0 {
        compacted.push(ChatMessage::system(
            "[Note: Earlier conversation history was automatically compacted \
             to fit within the context window. The most recent exchange is preserved below.]",
        ));
    }

    compacted.extend_from_slice(&messages[user_idx..]);
    compacted
}

/// Compact messages when no User message exists (edge case).
fn compact_without_user_message(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    use crate::llm::Role;
    let non_system_indices: Vec<_> = messages
        .iter()
        .enumerate()
        .filter_map(|(idx, message)| (message.role != Role::System).then_some(idx))
        .collect();
    let keep = if non_system_indices.len() >= 2 { 2 } else { 1 };
    let retained_non_system: std::collections::HashSet<_> =
        non_system_indices.into_iter().rev().take(keep).collect();
    messages
        .iter()
        .enumerate()
        .filter(|(idx, message)| message.role == Role::System || retained_non_system.contains(idx))
        .map(|(_, message)| message.clone())
        .collect()
}

/// Compact messages for retry after a context-length-exceeded error.
///
/// Keeps all `System` messages (which carry the system prompt and instructions),
/// finds the last `User` message, and retains it plus every subsequent message
/// (the current turn's assistant tool calls and tool results). A short note is
/// inserted so the LLM knows earlier history was dropped.
pub(crate) fn compact_messages_for_retry(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    use crate::llm::Role;
    match messages.iter().rposition(|m| m.role == Role::User) {
        Some(idx) => compact_around_user_message(messages, idx),
        None => compact_without_user_message(messages),
    }
}

/// Strip internal `[Called tool ...]` and `[Tool ... returned: ...]` markers
/// from a response string. These markers are inserted by provider-level message
/// flattening (e.g. NEAR AI) and can leak into the user-visible response when
/// the LLM echoes them back.
fn is_internal_tool_line(line: &str) -> bool {
    let trimmed = line.trim();
    (trimmed.starts_with("[Called tool ") && trimmed.ends_with(']'))
        || (trimmed.starts_with("[Tool ")
            && trimmed.contains(" returned:")
            && trimmed.ends_with(']'))
        || (trimmed.starts_with("[TOOL_CALL:") && trimmed.ends_with(']'))
}
pub(crate) fn strip_internal_tool_call_text(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    // Remove lines that are purely internal tool-call markers.
    // Pattern: lines matching `[Called tool <name>(...)]`,
    // `[Tool <name> returned: ...]`, or `[TOOL_CALL:<name>]`.
    let result = text
        .lines()
        .filter(|line| !is_internal_tool_line(line))
        .fold(String::new(), |mut acc, s| {
            if !acc.is_empty() {
                acc.push('\n');
            }
            acc.push_str(s);
            acc
        });

    let result = result.trim();
    if result.is_empty() {
        "I wasn't able to complete that request. Could you try rephrasing or providing more details?".to_string()
    } else {
        result.to_string()
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::llm::Role;

    const COMPACTION_NOTE: &str = concat!(
        "[Note: Earlier conversation history was automatically compacted ",
        "to fit within the context window. The most recent exchange is preserved below.]"
    );

    fn message(role: Role, content: String) -> ChatMessage {
        ChatMessage {
            role,
            content,
            content_parts: Vec::new(),
            tool_call_id: None,
            name: None,
            tool_calls: None,
        }
    }

    fn message_fingerprint(message: &ChatMessage) -> (Role, &str) {
        (message.role, message.content.as_str())
    }

    fn generated_message_strategy() -> impl Strategy<Value = ChatMessage> {
        (
            prop_oneof![
                Just(Role::System),
                Just(Role::User),
                Just(Role::Assistant),
                Just(Role::Tool),
            ],
            any::<String>(),
        )
            .prop_map(|(role, content)| message(role, content))
    }

    proptest! {
        #[test]
        fn compact_messages_for_retry_preserves_compaction_invariants(
            messages in prop::collection::vec(generated_message_strategy(), 0..32)
        ) {
            let compacted = compact_messages_for_retry(&messages);
            let compacted_without_note: Vec<_> = compacted
                .iter()
                .filter(|message| message.role != Role::System || message.content != COMPACTION_NOTE)
                .collect();

            let mut next_idx = 0usize;
            for compacted_message in &compacted_without_note {
                let fingerprint = message_fingerprint(compacted_message);
                let matched_idx = messages[next_idx..]
                    .iter()
                    .position(|original| message_fingerprint(original) == fingerprint)
                    .map(|offset| next_idx + offset);
                prop_assert!(
                    matched_idx.is_some(),
                    "compacted message {:?} should appear in original input after index {}",
                    fingerprint,
                    next_idx
                );
                next_idx = matched_idx.expect("position checked above") + 1;
            }

            if let Some(user_idx) = messages.iter().rposition(|message| message.role == Role::User) {
                let expected_suffix: Vec<_> = messages[user_idx..]
                    .iter()
                    .map(message_fingerprint)
                    .collect();
                let actual_suffix: Vec<_> = compacted_without_note
                    .iter()
                    .rev()
                    .take(expected_suffix.len())
                    .copied()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .map(message_fingerprint)
                    .collect();
                prop_assert_eq!(actual_suffix, expected_suffix);
            }

            for system_message in messages.iter().filter(|message| message.role == Role::System) {
                let original_count = messages
                    .iter()
                    .filter(|message| message_fingerprint(message) == message_fingerprint(system_message))
                    .count();
                let compacted_count = compacted
                    .iter()
                    .filter(|message| message_fingerprint(message) == message_fingerprint(system_message))
                    .count();
                prop_assert!(
                    compacted_count >= original_count,
                    "expected all system messages to remain present: {:?}",
                    message_fingerprint(system_message)
                );
            }

            let note_count = compacted
                .iter()
                .filter(|message| message.role == Role::System && message.content == COMPACTION_NOTE)
                .count();
            let truncation_occurred = messages
                .iter()
                .rposition(|message| message.role == Role::User)
                .is_some_and(|user_idx| user_idx > 0);

            prop_assert!(note_count <= 1, "compaction note inserted more than once");
            if note_count == 1 {
                prop_assert!(
                    truncation_occurred,
                    "compaction note should only appear when history before the preserved suffix was truncated"
                );
            }
        }
    }

    #[test]
    fn compact_keeps_all_system_messages() {
        let messages = vec![
            ChatMessage::system("system one"),
            ChatMessage::user("user"),
            ChatMessage::assistant("assistant"),
        ];

        let compacted = compact_messages_for_retry(&messages);

        assert!(
            compacted
                .iter()
                .any(|message| message.role == Role::System && message.content == "system one")
        );
    }

    #[test]
    fn compact_retains_last_user_and_tail() {
        let messages = vec![
            ChatMessage::system("system"),
            ChatMessage::user("first user"),
            ChatMessage::assistant("assistant"),
            ChatMessage::user("second user"),
            ChatMessage::tool_result("call-1", "echo", "tool output"),
        ];

        let compacted = compact_messages_for_retry(&messages);

        assert!(
            compacted
                .iter()
                .any(|message| message.role == Role::User && message.content == "second user")
        );
        assert!(compacted.iter().any(|message| {
            message.role == Role::Tool
                && message.name.as_deref() == Some("echo")
                && message.content == "tool output"
        }));
    }

    #[test]
    fn compact_without_user_message_preserves_system_first() {
        let messages = vec![
            ChatMessage::system("system"),
            ChatMessage::assistant("assistant"),
        ];

        let compacted = compact_messages_for_retry(&messages);

        assert_eq!(
            compacted.first().map(|message| message.role),
            Some(Role::System)
        );
    }

    #[test]
    fn strip_removes_bracketed_markers() {
        let text = "before\n[TOOL_CALL:foo]\nafter";

        let stripped = strip_internal_tool_call_text(text);

        assert!(!stripped.contains("[TOOL_CALL:foo]"));
    }

    #[test]
    fn strip_empty_string_returns_empty() {
        assert_eq!(strip_internal_tool_call_text(""), "");
    }

    #[test]
    fn strip_plain_text_unchanged() {
        let text = "plain text without internal markers";
        assert_eq!(strip_internal_tool_call_text(text), text);
    }
}
