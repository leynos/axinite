//! Types and constants for the dispatcher module.

use std::sync::Arc;

/// Maximum characters for tool output preview.
pub(crate) const PREVIEW_MAX_CHARS: usize = 1024;

/// Check if a string is valid JSON (object or array).
pub(crate) fn is_valid_json(s: &str) -> bool {
    let t = s.trim();
    if !(t.starts_with('{') || t.starts_with('[')) {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(t).is_ok()
}

/// Collapse a tool output string into a single-line preview for display.
pub(crate) fn truncate_for_preview(output: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let collapsed = output.split_whitespace().collect::<Vec<_>>().join(" ");
    let total = collapsed.chars().count();
    if total <= max_chars {
        return collapsed;
    }
    let mut truncated = String::with_capacity(max_chars + 3);
    truncated.extend(collapsed.chars().take(max_chars));
    truncated.push_str("...");
    truncated
}

/// Select active skills for a message using deterministic prefiltering.
pub(super) fn select_active_skills(
    registry: &Arc<std::sync::RwLock<crate::skills::SkillRegistry>>,
    skills_cfg: &crate::config::SkillsConfig,
    message_content: &str,
) -> Vec<crate::skills::LoadedSkill> {
    if !skills_cfg.enabled {
        return vec![];
    }
    let guard = match registry.read() {
        Ok(g) => g,
        Err(e) => {
            tracing::error!("Skill registry lock poisoned: {}", e);
            return vec![];
        }
    };
    let available = guard.skills();
    let selected = crate::skills::prefilter_skills(
        message_content,
        available,
        skills_cfg.max_active_skills,
        skills_cfg.max_context_tokens,
    );

    if !selected.is_empty() {
        tracing::debug!(
            "Selected {} skill(s) for message: {}",
            selected.len(),
            selected
                .iter()
                .map(|s| s.name())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    selected.into_iter().cloned().collect()
}

/// Result of the agentic loop execution.
pub(crate) enum AgenticLoopResult {
    /// Completed with a response.
    Response(String),
    /// A tool requires approval before continuing.
    NeedApproval {
        /// The pending approval request to store.
        pending: crate::agent::session::PendingApproval,
    },
}

/// Outcome of preflight check for a single tool call.
pub(super) enum PreflightOutcome {
    Rejected(String),
    Runnable,
}

/// Result of grouping tool calls into batches.
pub(super) struct ToolBatch {
    pub(super) preflight: Vec<(crate::llm::ToolCall, PreflightOutcome)>,
    pub(super) runnable: Vec<(usize, crate::llm::ToolCall)>,
}

/// Parsed auth result fields for emitting StatusUpdate::AuthRequired.
pub(crate) struct ParsedAuthData {
    pub(crate) auth_url: Option<String>,
    pub(crate) setup_url: Option<String>,
}

/// Extract auth_url and setup_url from a tool_auth result JSON string.
pub(crate) fn parse_auth_result(result: &Result<String, crate::error::Error>) -> ParsedAuthData {
    let parsed = result
        .as_ref()
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
    ParsedAuthData {
        auth_url: parsed
            .as_ref()
            .and_then(|v| v.get("auth_url"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        setup_url: parsed
            .as_ref()
            .and_then(|v| v.get("setup_url"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    }
}

/// Check if a tool_auth result indicates the extension is awaiting a token.
///
/// Returns `Some((extension_name, instructions))` if the tool result contains
/// `awaiting_token: true`, meaning the thread should enter auth mode.
pub(crate) fn check_auth_required(
    tool_name: &str,
    result: &Result<String, crate::error::Error>,
) -> Option<(String, String)> {
    if tool_name != "tool_auth" && tool_name != "tool_activate" {
        return None;
    }
    let output = result.as_ref().ok()?;
    let parsed: serde_json::Value = serde_json::from_str(output).ok()?;
    if parsed.get("awaiting_token") != Some(&serde_json::Value::Bool(true)) {
        return None;
    }
    let name = parsed.get("name")?.as_str()?.to_string();
    let instructions = parsed
        .get("instructions")
        .and_then(|v| v.as_str())
        .unwrap_or("Please provide your API token/key.")
        .to_string();
    Some((name, instructions))
}

/// Compact messages for retry after a context-length-exceeded error.
///
/// Keeps all `System` messages (which carry the system prompt and instructions),
/// finds the last `User` message, and retains it plus every subsequent message
/// (the current turn's assistant tool calls and tool results). A short note is
/// inserted so the LLM knows earlier history was dropped.
pub(super) fn compact_messages_for_retry(
    messages: &[crate::llm::ChatMessage],
) -> Vec<crate::llm::ChatMessage> {
    use crate::llm::Role;

    let mut compacted = Vec::new();

    // Find the last User message index
    let last_user_idx = messages.iter().rposition(|m| m.role == Role::User);

    if let Some(idx) = last_user_idx {
        // Keep System messages that appear BEFORE the last User message.
        // System messages after that point (e.g. nudges) are included in the
        // slice extension below, avoiding duplication.
        for msg in &messages[..idx] {
            if msg.role == Role::System {
                compacted.push(msg.clone());
            }
        }

        // Only add a compaction note if there was earlier history that is being dropped
        if idx > 0 {
            compacted.push(crate::llm::ChatMessage::system(
                "[Note: Earlier conversation history was automatically compacted \
                 to fit within the context window. The most recent exchange is preserved below.]",
            ));
        }

        // Keep the last User message and everything after it
        compacted.extend_from_slice(&messages[idx..]);
    } else {
        // No user messages found (shouldn't happen normally); keep everything,
        // with system messages first to preserve prompt ordering.
        for msg in messages {
            if msg.role == Role::System {
                compacted.push(msg.clone());
            }
        }
        for msg in messages {
            if msg.role != Role::System {
                compacted.push(msg.clone());
            }
        }
    }

    compacted
}

/// Strip internal `[Called tool ...]` and `[Tool ... returned: ...]` markers
/// from a response string. These markers are inserted by provider-level message
/// flattening (e.g. NEAR AI) and can leak into the user-visible response when
/// the LLM echoes them back.
pub(super) fn strip_internal_tool_call_text(text: &str) -> String {
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

/// Execute a chat tool without requiring `&Agent`.
///
/// This standalone function enables parallel invocation from spawned JoinSet
/// tasks, which cannot borrow `&self`. Delegates to the shared
/// `execute_tool_with_safety` pipeline.
pub(crate) async fn execute_chat_tool_standalone(
    tools: &crate::tools::ToolRegistry,
    safety: &crate::safety::SafetyLayer,
    tool_name: &str,
    params: &serde_json::Value,
    job_ctx: &crate::context::JobContext,
) -> Result<String, crate::error::Error> {
    crate::tools::execute::execute_tool_with_safety(tools, safety, tool_name, params, job_ctx).await
}

/// Execution context for tool calls.
#[expect(dead_code, reason = "scaffolding for future tool-exec refactor")]
pub(super) struct ExecCtx<'a> {
    pub(super) tools: &'a Arc<crate::tools::ToolRegistry>,
    pub(super) safety: &'a Arc<crate::safety::SafetyLayer>,
    pub(super) channels: &'a Arc<crate::channels::ChannelManager>,
    pub(super) channel: &'a str,
    pub(super) user_id: &'a str,
    pub(super) metadata: &'a serde_json::Value,
    pub(super) preview_limit: usize,
}
