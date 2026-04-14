use crate::agent::dispatcher::delegate::ChatDelegate;
use crate::channels::StatusUpdate;
use crate::error::Error;
use crate::llm::{ChatMessage, ReasoningContext};

use super::recording::record_tool_outcome;

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

/// Phase 3: iterate preflight outcomes in original order, dispatching each
/// to `handle_rejected_tool` or `process_runnable_tool`.
/// Returns the first deferred-auth instruction string, if any.
pub(super) async fn run_postflight(
    delegate: &ChatDelegate<'_>,
    preflight: Vec<(crate::llm::ToolCall, super::preflight::PreflightOutcome)>,
    exec_results: &mut [Option<Result<String, Error>>],
    reason_ctx: &mut ReasoningContext,
) -> Option<String> {
    let mut deferred_auth: Option<String> = None;
    for (pf_idx, (tc, outcome)) in preflight.into_iter().enumerate() {
        match outcome {
            super::preflight::PreflightOutcome::Rejected(error_msg) => {
                handle_rejected_tool(delegate, &tc, &error_msg, reason_ctx).await;
            }
            super::preflight::PreflightOutcome::Runnable => {
                let tool_result = exec_results[pf_idx].take().unwrap_or_else(|| {
                    Err(crate::error::ToolError::ExecutionFailed {
                        name: tc.name.clone(),
                        reason: "No result available".to_string(),
                    }
                    .into())
                });
                if let Some(instructions) =
                    process_runnable_tool(delegate, &tc, tool_result, reason_ctx).await
                {
                    deferred_auth = Some(instructions);
                    break;
                }
            }
        }
    }
    deferred_auth
}

/// Handle rejected tool call outcome.
pub(super) async fn handle_rejected_tool(
    delegate: &ChatDelegate<'_>,
    tc: &crate::llm::ToolCall,
    error_msg: &str,
    reason_ctx: &mut ReasoningContext,
) {
    {
        let mut sess = delegate.session.lock().await;
        if let Some(thread) = sess.threads.get_mut(&delegate.thread_id)
            && let Some(turn) = thread.last_turn_mut()
        {
            turn.record_tool_error(error_msg.to_string());
        }
    }
    reason_ctx.messages.push(ChatMessage::tool_result(
        &tc.id,
        &tc.name,
        error_msg.to_string(),
    ));
}

/// Process post-flight for a single runnable tool.
pub(super) async fn process_runnable_tool(
    delegate: &ChatDelegate<'_>,
    tc: &crate::llm::ToolCall,
    tool_result: Result<String, Error>,
    reason_ctx: &mut ReasoningContext,
) -> Option<String> {
    use crate::agent::dispatcher::{PREVIEW_MAX_CHARS, is_valid_json, truncate_for_preview};

    let is_tool_error = tool_result.is_err();

    let output = match &tool_result {
        Ok(output) => output,
        Err(e) => {
            let error_msg = format!("Tool '{}' failed: {}", tc.name, e);
            fold_into_context(
                delegate,
                tc,
                ToolOutcome {
                    result_content: error_msg,
                    is_tool_error: true,
                },
                reason_ctx,
            )
            .await;
            return None;
        }
    };

    let is_image_sentinel = maybe_emit_image_sentinel(delegate, &tc.name, output).await;
    let image_sentinel_summary = image_sentinel_summary(output);

    let (result_content, preview) = if is_image_sentinel {
        let summary = image_sentinel_summary.unwrap_or_else(|| "[Image generated]".to_string());
        (summary.clone(), summary)
    } else if is_valid_json(output) {
        let preview = truncate_for_preview(output, PREVIEW_MAX_CHARS);
        (output.clone(), preview)
    } else {
        let (preview_text, wrapped_text) = sanitize_output(delegate, &tc.name, output);
        let preview = truncate_for_preview(&preview_text, PREVIEW_MAX_CHARS);
        (wrapped_text, preview)
    };

    if !is_image_sentinel && !preview.is_empty() {
        let _ = delegate
            .agent
            .channels
            .send_status(
                &delegate.message.channel,
                StatusUpdate::ToolResult {
                    name: tc.name.clone(),
                    preview,
                },
                &delegate.message.metadata,
            )
            .await;
    }

    let auth_instructions =
        if let Some((ext_name, instructions)) = check_auth_required(&tc.name, &tool_result) {
            let auth_data = parse_auth_result(&tool_result);
            {
                let mut sess = delegate.session.lock().await;
                if let Some(thread) = sess.threads.get_mut(&delegate.thread_id) {
                    thread.enter_auth_mode(ext_name.clone());
                }
            }
            let _ = delegate
                .agent
                .channels
                .send_status(
                    &delegate.message.channel,
                    StatusUpdate::AuthRequired {
                        extension_name: ext_name,
                        instructions: Some(instructions.clone()),
                        auth_url: auth_data.auth_url,
                        setup_url: auth_data.setup_url,
                    },
                    &delegate.message.metadata,
                )
                .await;
            Some(instructions)
        } else {
            None
        };

    delegate
        .job_ctx
        .tool_output_stash
        .write()
        .await
        .insert(tc.id.clone(), output.clone());

    fold_into_context(
        delegate,
        tc,
        ToolOutcome {
            result_content,
            is_tool_error,
        },
        reason_ctx,
    )
    .await;

    auth_instructions
}

/// Emit image sentinel status update if applicable.
async fn maybe_emit_image_sentinel(
    delegate: &ChatDelegate<'_>,
    tool_name: &str,
    output: &str,
) -> bool {
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
            let _ = delegate
                .agent
                .channels
                .send_status(
                    &delegate.message.channel,
                    StatusUpdate::ImageGenerated { data_url, path },
                    &delegate.message.metadata,
                )
                .await;
        }
        return true;
    }
    false
}

fn image_sentinel_summary(output: &str) -> Option<String> {
    let sentinel = serde_json::from_str::<serde_json::Value>(output).ok()?;
    if sentinel.get("type").and_then(|value| value.as_str()) != Some("image_generated") {
        return None;
    }

    let mut parts = vec!["[Image generated]".to_string()];
    if let Some(media_type) = sentinel.get("media_type").and_then(|value| value.as_str()) {
        parts.push(format!("type={media_type}"));
    }
    if let Some(size) = sentinel.get("size").and_then(|value| value.as_str()) {
        parts.push(format!("size={size}"));
    }
    if let Some(path) = sentinel.get("path").and_then(|value| value.as_str()) {
        parts.push(format!("path={path}"));
    } else if let Some(source_path) = sentinel.get("source_path").and_then(|value| value.as_str()) {
        parts.push(format!("source={source_path}"));
    }
    Some(parts.join(" "))
}

/// Sanitize tool output and return both preview text (raw sanitized) and wrapped text (for LLM).
fn sanitize_output(delegate: &ChatDelegate<'_>, tool_name: &str, output: &str) -> (String, String) {
    let sanitized = delegate
        .agent
        .safety()
        .sanitize_tool_output(tool_name, output);
    let preview_text = sanitized.content.clone();
    let wrapped_text =
        delegate
            .agent
            .safety()
            .wrap_for_llm(tool_name, &sanitized.content, sanitized.was_modified);
    (preview_text, wrapped_text)
}

/// Outcome of a tool execution for folding into context.
pub(super) struct ToolOutcome {
    pub(super) result_content: String,
    pub(super) is_tool_error: bool,
}

/// Fold tool result into context messages.
pub(super) async fn fold_into_context(
    delegate: &ChatDelegate<'_>,
    tc: &crate::llm::ToolCall,
    outcome: ToolOutcome,
    reason_ctx: &mut ReasoningContext,
) {
    record_tool_outcome(
        delegate,
        &tc.name,
        &outcome.result_content,
        outcome.is_tool_error,
    )
    .await;

    reason_ctx.messages.push(ChatMessage::tool_result(
        &tc.id,
        &tc.name,
        outcome.result_content,
    ));
}
