//! Standalone tool execution and tool-auth result parsing.
//!
//! Provides the entry points the dispatcher and approval flows use to run a
//! chat tool without holding `&Agent`, plus the helpers that interpret
//! `tool_auth` results for the approval and auth flows.

use crate::agent::Agent;
use crate::context::JobContext;
use crate::error::Error;

/// Describes a single tool invocation passed to `execute_chat_tool_standalone`.
pub(crate) struct ChatToolRequest<'a> {
    pub(crate) tool_name: &'a str,
    pub(crate) params: &'a serde_json::Value,
}

/// Execute a chat tool without requiring `&Agent`.
///
/// This standalone function enables parallel invocation from spawned JoinSet
/// tasks, which cannot borrow `&self`. Delegates to the shared
/// `execute_tool_with_safety` pipeline.
pub(crate) async fn execute_chat_tool_standalone(
    tools: &crate::tools::ToolRegistry,
    safety: &crate::safety::SafetyLayer,
    request: &ChatToolRequest<'_>,
    job_ctx: &JobContext,
) -> Result<String, Error> {
    crate::tools::execute::execute_tool_with_safety(
        tools,
        safety,
        request.tool_name,
        request.params,
        job_ctx,
    )
    .await
}

/// Parsed auth result fields for emitting StatusUpdate::AuthRequired.
pub(crate) struct ParsedAuthData {
    pub(crate) auth_url: Option<String>,
    pub(crate) setup_url: Option<String>,
}

/// Extract auth_url and setup_url from a tool_auth result JSON string.
pub(crate) fn parse_auth_result(result: &Result<String, Error>) -> ParsedAuthData {
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
    result: &Result<String, Error>,
) -> Option<(String, String)> {
    if tool_name != "tool_auth" && tool_name != "tool_activate" {
        return None;
    }
    let output = result.as_ref().ok()?;
    let parsed: serde_json::Value = serde_json::from_str(output).ok()?;
    let awaiting_token = parsed.get("awaiting_token") == Some(&serde_json::Value::Bool(true))
        || parsed.get("type").and_then(|value| value.as_str()) == Some("awaiting_token");
    if !awaiting_token {
        return None;
    }
    let name = parsed
        .get("name")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| tool_name.to_string());
    let instructions = parsed
        .get("instructions")
        .and_then(|v| v.as_str())
        .unwrap_or("Please provide your API token/key.")
        .to_string();
    Some((name, instructions))
}

impl Agent {
    /// Execute a tool for chat (without full job context).
    pub(crate) async fn execute_chat_tool(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
        job_ctx: &JobContext,
    ) -> Result<String, Error> {
        execute_chat_tool_standalone(
            self.tools(),
            self.safety(),
            &ChatToolRequest { tool_name, params },
            job_ctx,
        )
        .await
    }
}
