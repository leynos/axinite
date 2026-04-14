//! Tool execution logic for the chat delegate.
//!
//! Splits the 3-phase tool execution pipeline into cohesive submodules:
//! preflight, execution, postflight, and recording.

pub mod execution;
pub mod postflight;
pub mod preflight;
pub mod recording;

use uuid::Uuid;

use crate::agent::dispatcher::delegate::ChatDelegate;
use crate::agent::session::PendingApproval;
use crate::channels::StatusUpdate;
use crate::error::Error;
use crate::llm::{ChatMessage, ReasoningContext};

pub(crate) use execution::ToolCallSpec;
pub(crate) use execution::execute_chat_tool_standalone;
pub(crate) use postflight::{check_auth_required, parse_auth_result};

fn build_pending_approval(
    delegate: &ChatDelegate<'_>,
    candidate: preflight::ApprovalCandidate,
    tool_calls: &[crate::llm::ToolCall],
    reason_ctx: &ReasoningContext,
) -> PendingApproval {
    let display_params = crate::tools::redact_params(
        &candidate.tool_call.arguments,
        candidate.tool.sensitive_params(),
    );
    PendingApproval {
        request_id: Uuid::new_v4(),
        tool_name: candidate.tool_call.name.clone(),
        parameters: candidate.tool_call.arguments.clone(),
        display_parameters: display_params,
        description: candidate.tool.description().to_string(),
        tool_call_id: candidate.tool_call.id.clone(),
        context_messages: reason_ctx.messages.clone(),
        deferred_tool_calls: tool_calls[candidate.idx + 1..].to_vec(),
        user_timezone: Some(delegate.user_tz.name().to_string()),
    }
}

fn finalized_tool_calls(
    original_tool_calls: &[crate::llm::ToolCall],
    preflight: &[(crate::llm::ToolCall, preflight::PreflightOutcome)],
    approval_needed: Option<&preflight::ApprovalCandidate>,
) -> Vec<crate::llm::ToolCall> {
    let mut finalized = preflight
        .iter()
        .map(|(tc, _)| tc.clone())
        .collect::<Vec<_>>();
    if let Some(candidate) = approval_needed {
        finalized.push(candidate.tool_call.clone());
        finalized.extend_from_slice(&original_tool_calls[candidate.idx + 1..]);
    }
    finalized
}

/// Execute tool calls with 3-phase pipeline (preflight → execution → post-flight).
pub(crate) async fn execute_tool_calls(
    delegate: &ChatDelegate<'_>,
    tool_calls: Vec<crate::llm::ToolCall>,
    content: Option<String>,
    reason_ctx: &mut ReasoningContext,
) -> Result<Option<crate::agent::agentic_loop::LoopOutcome>, Error> {
    use crate::agent::agentic_loop::LoopOutcome;

    let (batch, approval_needed) = preflight::group_tool_calls(delegate, &tool_calls).await?;
    let preflight::ToolBatch {
        preflight,
        runnable,
    } = batch;
    let finalized_tool_calls =
        finalized_tool_calls(&tool_calls, &preflight, approval_needed.as_ref());

    reason_ctx
        .messages
        .push(ChatMessage::assistant_with_tool_calls(
            content,
            finalized_tool_calls.clone(),
        ));

    let _ = delegate
        .agent
        .channels
        .send_status(
            &delegate.message.channel,
            StatusUpdate::Thinking(format!("Executing {} tool(s)...", tool_calls.len())),
            &delegate.message.metadata,
        )
        .await;

    recording::record_redacted_tool_calls(delegate, &finalized_tool_calls).await;

    let mut exec_results = execution::run_phase2(delegate, preflight.len(), &runnable).await;
    let deferred_auth =
        postflight::run_postflight(delegate, preflight, &mut exec_results, reason_ctx).await;

    if let Some(candidate) = approval_needed {
        let pending =
            build_pending_approval(delegate, candidate, &finalized_tool_calls, reason_ctx);
        return Ok(Some(LoopOutcome::NeedApproval(Box::new(pending))));
    }

    if let Some(instructions) = deferred_auth {
        return Ok(Some(LoopOutcome::Response(instructions)));
    }

    Ok(None)
}
