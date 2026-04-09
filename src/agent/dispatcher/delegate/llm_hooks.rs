//! LLM hook implementations for the chat delegate.
//!
//! Contains the LLM call hooks (check_signals, before_llm_call, call_llm,
//! handle_text_response) and helper functions for message compaction and
//! response sanitization.

use crate::agent::agentic_loop::{LoopOutcome, LoopSignal, TextAction};
use crate::agent::dispatcher::delegate::ChatDelegate;
use crate::agent::session::ThreadState;
use crate::channels::StatusUpdate;
use crate::error::Error;
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
    reason_ctx.available_tools = tool_defs;
    reason_ctx.system_prompt = Some(if force_text {
        delegate.cached_prompt_no_tools.clone()
    } else {
        delegate.cached_prompt.clone()
    });
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
    // Enforce cost guardrails before the LLM call
    if let Err(limit) = delegate.agent.cost_guard().check_allowed().await {
        return Err(crate::error::LlmError::InvalidResponse {
            provider: "agent".to_string(),
            reason: limit.to_string(),
        }
        .into());
    }

    let output = match reasoning.respond_with_tools(reason_ctx).await {
        Ok(output) => output,
        Err(crate::error::LlmError::ContextLengthExceeded { used, limit }) => {
            tracing::warn!(
                used,
                limit,
                iteration,
                "Context length exceeded, compacting messages and retrying"
            );

            // Compact messages in place and retry
            reason_ctx.messages = compact_messages_for_retry(&reason_ctx.messages);

            // When force_text, clear tools to further reduce token count
            if reason_ctx.force_text {
                reason_ctx.available_tools.clear();
            }

            reasoning
                .respond_with_tools(reason_ctx)
                .await
                .map_err(|retry_err| {
                    tracing::error!(
                        original_used = used,
                        original_limit = limit,
                        retry_error = %retry_err,
                        "Retry after auto-compaction also failed"
                    );
                    crate::error::Error::from(retry_err)
                })?
        }
        Err(e) => return Err(e.into()),
    };

    // Record cost and track token usage
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

    Ok(output)
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
    let mut compacted = collect_system_messages(messages);
    compacted.extend(messages.iter().filter(|m| m.role != Role::System).cloned());
    compacted
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
pub(crate) fn strip_internal_tool_call_text(text: &str) -> String {
    // Remove lines that are purely internal tool-call markers.
    // Pattern: lines matching `[Called tool <name>(...)]` or `[Tool <name> returned: ...]`
    let result = text
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !((trimmed.starts_with("[Called tool ") && trimmed.ends_with(']'))
                || (trimmed.starts_with("[Tool ")
                    && trimmed.contains(" returned:")
                    && trimmed.ends_with(']')))
        })
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
