//! Post-flight processing for tool execution.
//!
//! Contains the post-flight phase logic for sanitizing outputs, recording
//! outcomes, folding results into context, and handling auth requirements.

use crate::agent::dispatcher::delegate::ChatDelegate;
use crate::agent::dispatcher::{PREVIEW_MAX_CHARS, is_valid_json, truncate_for_preview};
use crate::channels::StatusUpdate;
use crate::error::Error;
use crate::llm::{ChatMessage, ReasoningContext};

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

impl<'a> ChatDelegate<'a> {
    /// Sanitize tool output and return both preview text (raw sanitized) and wrapped text (for LLM).
    pub(super) fn sanitize_output(&self, tool_name: &str, output: &str) -> (String, String) {
        let sanitized = self.agent.safety().sanitize_tool_output(tool_name, output);
        let preview_text = sanitized.content.clone();
        let wrapped_text =
            self.agent
                .safety()
                .wrap_for_llm(tool_name, &sanitized.content, sanitized.was_modified);
        (preview_text, wrapped_text)
    }

    /// Record tool outcome in the thread.
    pub(super) async fn record_tool_outcome(
        &self,
        _tool_name: &str,
        result_content: &str,
        is_tool_error: bool,
    ) {
        let mut sess = self.session.lock().await;
        if let Some(thread) = sess.threads.get_mut(&self.thread_id)
            && let Some(turn) = thread.last_turn_mut()
        {
            if is_tool_error {
                turn.record_tool_error(result_content.to_string());
            } else {
                turn.record_tool_result(serde_json::json!(result_content));
            }
        }
    }

    /// Emit image sentinel status update if applicable.
    pub(super) async fn maybe_emit_image_sentinel(&self, tool_name: &str, output: &str) -> bool {
        if !matches!(tool_name, "image_generate" | "image_edit") {
            return false;
        }

        if let Ok(sentinel) = serde_json::from_str::<serde_json::Value>(output)
            && sentinel.get("type").and_then(|v| v.as_str()) == Some("image_generated")
        {
            let data_url = sentinel
                .get("data")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let path = sentinel
                .get("path")
                .and_then(|v| v.as_str())
                .map(String::from);
            if data_url.is_empty() {
                tracing::warn!("Image generation sentinel has empty data URL, skipping broadcast");
            } else {
                let _ = self
                    .agent
                    .channels
                    .send_status(
                        &self.message.channel,
                        StatusUpdate::ImageGenerated { data_url, path },
                        &self.message.metadata,
                    )
                    .await;
            }
            return true;
        }
        false
    }

    /// Fold tool result into context messages.
    pub(super) async fn fold_into_context(
        &self,
        tc: &crate::llm::ToolCall,
        result_content: String,
        is_tool_error: bool,
        reason_ctx: &mut ReasoningContext,
    ) {
        // Record sanitized result in thread
        self.record_tool_outcome(&tc.name, &result_content, is_tool_error)
            .await;

        reason_ctx
            .messages
            .push(ChatMessage::tool_result(&tc.id, &tc.name, result_content));
    }

    /// Process post-flight for a single runnable tool.
    pub(super) async fn process_runnable_tool(
        &self,
        tc: &crate::llm::ToolCall,
        tool_result: Result<String, Error>,
        reason_ctx: &mut ReasoningContext,
    ) -> Option<String> {
        let is_tool_error = tool_result.is_err();

        // Handle error case early
        let output = match &tool_result {
            Ok(output) => output,
            Err(e) => {
                let error_msg = format!("Tool '{}' failed: {}", tc.name, e);
                self.fold_into_context(tc, error_msg, true, reason_ctx)
                    .await;
                return None;
            }
        };

        // Detect image generation sentinel
        let is_image_sentinel = self.maybe_emit_image_sentinel(&tc.name, output).await;

        // Determine result content and preview based on whether output is valid JSON
        let (result_content, preview) = if is_valid_json(output) {
            // For JSON-producing tools, persist raw JSON without wrapping
            let preview = truncate_for_preview(output, PREVIEW_MAX_CHARS);
            (output.clone(), preview)
        } else {
            // Sanitize tool output first (before sending preview or using in context)
            // preview_text is raw sanitized for preview, wrapped_text is for LLM context
            let (preview_text, wrapped_text) = self.sanitize_output(&tc.name, output);
            let preview = truncate_for_preview(&preview_text, PREVIEW_MAX_CHARS);
            (wrapped_text, preview)
        };

        // Send ToolResult preview
        if !is_image_sentinel && !preview.is_empty() {
            let _ = self
                .agent
                .channels
                .send_status(
                    &self.message.channel,
                    StatusUpdate::ToolResult {
                        name: tc.name.clone(),
                        preview,
                    },
                    &self.message.metadata,
                )
                .await;
        }

        // Check for auth awaiting (use original tool_result for auth detection)
        let auth_instructions =
            if let Some((ext_name, instructions)) = check_auth_required(&tc.name, &tool_result) {
                let auth_data = parse_auth_result(&tool_result);
                {
                    let mut sess = self.session.lock().await;
                    if let Some(thread) = sess.threads.get_mut(&self.thread_id) {
                        thread.enter_auth_mode(ext_name.clone());
                    }
                }
                let _ = self
                    .agent
                    .channels
                    .send_status(
                        &self.message.channel,
                        StatusUpdate::AuthRequired {
                            extension_name: ext_name,
                            instructions: Some(instructions.clone()),
                            auth_url: auth_data.auth_url,
                            setup_url: auth_data.setup_url,
                        },
                        &self.message.metadata,
                    )
                    .await;
                Some(instructions)
            } else {
                None
            };

        // Stash full output so subsequent tools can reference it
        self.job_ctx
            .tool_output_stash
            .write()
            .await
            .insert(tc.id.clone(), output.clone());

        // Fold result into context
        self.fold_into_context(tc, result_content, is_tool_error, reason_ctx)
            .await;

        auth_instructions
    }
}
