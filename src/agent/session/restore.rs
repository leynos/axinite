//! Helpers for rebuilding thread turns from checkpoint message sequences.

use crate::llm::ChatMessage;

use super::turn::Turn;

/// Peekable iterator over checkpoint messages being restored into turns.
pub(super) type MessageIter = std::iter::Peekable<std::vec::IntoIter<ChatMessage>>;

/// Consume tool call sequences (assistant_with_tool_calls + tool_results),
/// recording them against the turn being rebuilt. A single turn may contain
/// multiple rounds of tool calls, so the cumulative base index into
/// `turn.tool_calls` is tracked per round.
pub(super) fn consume_tool_call_rounds(iter: &mut MessageIter, turn: &mut Turn) {
    while next_opens_tool_call_round(iter) {
        let call_base_idx = turn.tool_calls.len();
        record_round_tool_calls(iter, turn);
        consume_tool_results(iter, turn, call_base_idx);
    }
}

/// Return `true` when the next message opens a tool-call round (an assistant
/// message carrying tool calls).
fn next_opens_tool_call_round(iter: &mut MessageIter) -> bool {
    iter.peek()
        .is_some_and(|next| next.role == crate::llm::Role::Assistant && next.tool_calls.is_some())
}

/// Consume the round's assistant message, recording each tool call it made.
fn record_round_tool_calls(iter: &mut MessageIter, turn: &mut Turn) {
    if let Some(assistant_msg) = iter.next()
        && let Some(ref tcs) = assistant_msg.tool_calls
    {
        for tc in tcs {
            turn.record_tool_call(&tc.name, tc.arguments.clone());
        }
    }
}

/// Consume the tool_result messages for one round of tool calls, indexing
/// relative to the batch's base offset within `turn.tool_calls`.
fn consume_tool_results(iter: &mut MessageIter, turn: &mut Turn, call_base_idx: usize) {
    let mut pos = 0;
    while next_is_tool_result(iter) {
        if let Some(tool_msg) = iter.next() {
            record_tool_result(turn, call_base_idx + pos, &tool_msg);
        }
        pos += 1;
    }
}

/// Return `true` when the next message is a tool result.
fn next_is_tool_result(iter: &mut MessageIter) -> bool {
    iter.peek()
        .is_some_and(|tr| tr.role == crate::llm::Role::Tool)
}

/// Store the tool result content on the matching recorded call, if any.
fn record_tool_result(turn: &mut Turn, idx: usize, tool_msg: &ChatMessage) {
    if idx >= turn.tool_calls.len() {
        return;
    }
    // Store as result — the error/success distinction is for the live turn
    // only; restored context just needs the content the LLM originally saw.
    turn.tool_calls[idx].result = Some(serde_json::Value::String(tool_msg.content.clone()));
}

/// Complete the turn with the next message if it is the final assistant
/// response (a plain assistant message with no tool calls).
pub(super) fn consume_final_response(iter: &mut MessageIter, turn: &mut Turn) {
    let is_final_assistant = iter
        .peek()
        .is_some_and(|n| n.role == crate::llm::Role::Assistant && n.tool_calls.is_none());
    if is_final_assistant && let Some(response) = iter.next() {
        turn.complete(&response.content);
    }
}
