//! Postflight stage for chat tool execution.
//!
//! Interprets tool results, emits auth and image side effects, and folds each
//! indexed outcome back into both thread history and the reasoning context.

use crate::agent::dispatcher::delegate::ChatDelegate;
use crate::channels::StatusUpdate;
use crate::error::Error;
use crate::llm::{ChatMessage, ReasoningContext};

use super::execution::is_auth_barrier_tool;
use super::recording::record_tool_outcome;

/// Parsed auth result fields for emitting StatusUpdate::AuthRequired.
pub(crate) struct AuthBarrierData {
    pub(crate) extension_name: String,
    pub(crate) instructions: String,
    pub(crate) auth_url: Option<String>,
    pub(crate) setup_url: Option<String>,
}

pub(super) struct ToolCtx<'a> {
    pub(super) pf_idx: usize,
    pub(super) tc: &'a crate::llm::ToolCall,
}

/// Parse auth-barrier details from a tool_auth/tool_activate result.
pub(crate) fn parse_auth_barrier(
    tool_name: &str,
    result: &Result<String, Error>,
) -> Option<AuthBarrierData> {
    if !is_auth_barrier_tool(tool_name) {
        return None;
    }
    let output = result.as_ref().ok()?;
    let parsed: serde_json::Value = serde_json::from_str(output).ok()?;
    if parsed.get("awaiting_token") != Some(&serde_json::Value::Bool(true)) {
        return None;
    }
    let extension_name = parsed.get("name")?.as_str()?.to_string();
    let instructions = parsed
        .get("instructions")
        .and_then(|v| v.as_str())
        .unwrap_or("Please provide your API token/key.")
        .to_string();
    let auth_url = parsed
        .get("auth_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let setup_url = parsed
        .get("setup_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Some(AuthBarrierData {
        extension_name,
        instructions,
        auth_url,
        setup_url,
    })
}

pub(crate) fn check_auth_required(
    tool_name: &str,
    result: &Result<String, Error>,
) -> Option<(String, String)> {
    let auth_barrier = parse_auth_barrier(tool_name, result)?;
    Some((auth_barrier.extension_name, auth_barrier.instructions))
}

async fn handle_auth_barrier(
    delegate: &ChatDelegate<'_>,
    tc: &crate::llm::ToolCall,
    tool_result: &Result<String, Error>,
) -> Option<String> {
    let auth_barrier = parse_auth_barrier(&tc.name, tool_result)?;
    {
        let mut sess = delegate.session.lock().await;
        if let Some(thread) = sess.threads.get_mut(&delegate.thread_id) {
            thread.enter_auth_mode(auth_barrier.extension_name.clone());
        }
    }
    let _ = delegate
        .agent
        .channels
        .send_status(
            &delegate.message.channel,
            StatusUpdate::AuthRequired {
                extension_name: auth_barrier.extension_name,
                instructions: Some(auth_barrier.instructions.clone()),
                auth_url: auth_barrier.auth_url,
                setup_url: auth_barrier.setup_url,
            },
            &delegate.message.metadata,
        )
        .await;
    Some(auth_barrier.instructions)
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
                handle_rejected_tool(
                    delegate,
                    ToolCtx { pf_idx, tc: &tc },
                    &error_msg,
                    reason_ctx,
                )
                .await;
            }
            super::preflight::PreflightOutcome::Runnable => {
                let tool_result = exec_results
                    .get_mut(pf_idx)
                    .and_then(Option::take)
                    .unwrap_or_else(|| {
                        Err(crate::error::ToolError::ExecutionFailed {
                            name: tc.name.clone(),
                            reason: "No result available".to_string(),
                        }
                        .into())
                    });
                if let Some(instructions) =
                    process_runnable_tool(delegate, pf_idx, &tc, tool_result, reason_ctx).await
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
    tool: ToolCtx<'_>,
    error_msg: &str,
    reason_ctx: &mut ReasoningContext,
) {
    record_tool_outcome(delegate, tool.pf_idx, error_msg, true).await;
    reason_ctx.messages.push(ChatMessage::tool_result(
        &tool.tc.id,
        &tool.tc.name,
        error_msg.to_string(),
    ));
}

/// Process post-flight for a single runnable tool.
pub(super) async fn process_runnable_tool(
    delegate: &ChatDelegate<'_>,
    pf_idx: usize,
    tc: &crate::llm::ToolCall,
    tool_result: Result<String, Error>,
    reason_ctx: &mut ReasoningContext,
) -> Option<String> {
    use crate::agent::dispatcher::{PREVIEW_MAX_CHARS, truncate_for_preview};

    let is_tool_error = tool_result.is_err();

    let output = match &tool_result {
        Ok(output) => output,
        Err(e) => {
            let error_msg = format!("Tool '{}' failed: {}", tc.name, e);
            let (preview_text, wrapped_text) = sanitize_output(delegate, &tc.name, &error_msg);
            fold_into_context(
                delegate,
                ToolCtx { pf_idx, tc },
                ToolOutcome {
                    result_content: wrapped_text,
                    is_tool_error: true,
                },
                reason_ctx,
            )
            .await;
            let preview = truncate_for_preview(&preview_text, PREVIEW_MAX_CHARS);
            if !preview.is_empty() {
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
            return None;
        }
    };

    let is_image_sentinel = maybe_emit_image_sentinel(delegate, &tc.name, output).await;
    let image_sentinel_summary = image_sentinel_summary(output);

    let (result_content, preview) = if is_image_sentinel {
        let summary = image_sentinel_summary.unwrap_or_else(|| "[Image generated]".to_string());
        let (preview_text, wrapped_text) = sanitize_output(delegate, &tc.name, &summary);
        let preview = truncate_for_preview(&preview_text, PREVIEW_MAX_CHARS);
        (wrapped_text, preview)
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

    let auth_instructions = handle_auth_barrier(delegate, tc, &tool_result).await;

    // Stash raw `output` by `tc.id` for auditing/debugging while the LLM sees a separately sanitised form.
    delegate
        .job_ctx
        .tool_output_stash
        .write()
        .await
        .insert(tc.id.clone(), output.clone());

    fold_into_context(
        delegate,
        ToolCtx { pf_idx, tc },
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
pub(in crate::agent::dispatcher::delegate) async fn maybe_emit_image_sentinel(
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
    tool: ToolCtx<'_>,
    outcome: ToolOutcome,
    reason_ctx: &mut ReasoningContext,
) {
    record_tool_outcome(
        delegate,
        tool.pf_idx,
        &outcome.result_content,
        outcome.is_tool_error,
    )
    .await;

    reason_ctx.messages.push(ChatMessage::tool_result(
        &tool.tc.id,
        &tool.tc.name,
        outcome.result_content,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_auth_barrier_returns_urls_when_present() {
        let result = Ok(
            r#"{"awaiting_token":true,"name":"ngrok","instructions":"visit https://example.com","auth_url":"https://example.com/auth","setup_url":"https://example.com/setup"}"#
                .to_string(),
        );

        let parsed =
            parse_auth_barrier("tool_auth", &result).expect("auth barrier payload should parse");

        assert_eq!(
            parsed.auth_url,
            Some("https://example.com/auth".to_string())
        );
        assert_eq!(
            parsed.setup_url,
            Some("https://example.com/setup".to_string())
        );
    }

    #[test]
    fn parse_auth_barrier_returns_none_for_err_result() {
        let result = Err(crate::error::ToolError::ExecutionFailed {
            name: "tool_auth".to_string(),
            reason: "boom".to_string(),
        }
        .into());

        assert!(parse_auth_barrier("tool_auth", &result).is_none());
    }

    #[test]
    fn check_auth_required_returns_none_for_plain_output() {
        let result = Ok("plain output".to_string());

        assert!(check_auth_required("tool_auth", &result).is_none());
    }

    #[test]
    fn check_auth_required_returns_some_for_awaiting_token() {
        let payload =
            r#"{"awaiting_token":true,"name":"ngrok","instructions":"visit https://x.com"}"#;
        let result = Ok(payload.to_string());

        let (extension_name, instructions) = check_auth_required("tool_auth", &result)
            .expect("awaiting token payload should require auth");

        assert_eq!(extension_name, "ngrok");
        assert!(instructions.contains("visit"));
    }
}
