//! LLM reasoning capabilities for planning, tool selection, and evaluation.
//!
//! This module is split into focused submodules:
//! - [`cleaning`] — the `clean_response` pipeline and its tag regexes
//! - [`code_regions`] — fenced/inline code detection for code-aware stripping
//! - [`context`] — `ReasoningContext` and reasoning input/output types
//! - [`engine`] — `Reasoning` construction, builders, and `complete`
//! - [`intent`] — tool-intent and silent-reply detection
//! - [`planning`] — plan/select/evaluate flows and JSON extraction
//! - [`prompt`] — system prompt assembly and system-message merging
//! - [`recovery`] — recovering tool calls emitted as text
//! - [`respond`] — `respond` / `respond_with_tools`
//! - [`tag_stripping`] — code-aware reasoning/final/tool tag stripping
//! - [`truncation`] — truncation at unclosed tool-call tags (issue #789)

mod cleaning;
mod code_regions;
mod context;
mod engine;
mod intent;
mod planning;
mod prompt;
mod recovery;
mod respond;
mod tag_stripping;
mod truncation;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use crate::llm::LlmProvider;

pub use context::{
    ActionPlan, ReasoningContext, RespondOutput, RespondResult, SuccessEvaluation, TokenUsage,
    ToolSelection,
};
pub use intent::{is_silent_reply, llm_signals_tool_intent};

use cleaning::{FINAL_TAG_RE, PIPE_REASONING_TAG_RE, THINKING_TAG_RE, clean_response};
use code_regions::{CodeRegion, find_code_regions, is_inside_code};
use prompt::merge_system_messages;
use recovery::recover_tool_calls_from_content;
use tag_stripping::{
    extract_final_content, strip_pipe_reasoning_tags, strip_pipe_tag, strip_thinking_tags_regex,
    strip_xml_tag,
};
use truncation::truncate_at_tool_tags;

/// Token the agent returns when it has nothing to say (e.g. in group chats).
/// The dispatcher should check for this and suppress the message.
pub const SILENT_REPLY_TOKEN: &str = "NO_REPLY";

/// Nudge message injected when the LLM expresses intent to use a tool but
/// doesn't include any `tool_calls` in its response.
pub const TOOL_INTENT_NUDGE: &str = "\
You said you would perform an action, but you did not include any tool calls.\n\
Do NOT describe what you intend to do — actually call the tool now.\n\
Use the tool_calls mechanism to invoke the appropriate tool.";

/// Reasoning engine for the agent.
pub struct Reasoning {
    llm: Arc<dyn LlmProvider>,
    /// Optional workspace for loading identity/system prompts.
    workspace_system_prompt: Option<String>,
    /// Optional skill context block to inject into system prompt.
    skill_context: Option<String>,
    /// Channel name (e.g. "discord", "telegram") for formatting hints.
    channel: Option<String>,
    /// Model name for runtime context.
    model_name: Option<String>,
    /// Whether this is a group chat context.
    is_group_chat: bool,
    /// Channel-specific conversation context (e.g., sender number, UUID, group ID).
    /// This is passed to the LLM to provide clarity about who/group it's talking to.
    conversation_context: std::collections::HashMap<String, String>,
}
