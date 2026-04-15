//! Recording and post-flight phase for `ChatDelegate`.
//! Persists tool outcomes to the active thread, emits sanitised previews, and
//! folds ordered tool results back into reasoning context without panicking.

use crate::channels::StatusUpdate;
use crate::error::Error;
use crate::llm::{ChatMessage, ReasoningContext};

use super::ChatDelegate;
use crate::agent::dispatcher::types::*;

impl<'a> ChatDelegate<'a> {
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

    /// Fold tool result into context messages.
    pub(super) async fn fold_into_context(
        &self,
        tc: &crate::llm::ToolCall,
        outcome: ToolExecutionOutcome,
        reason_ctx: &mut ReasoningContext,
    ) {
        self.record_tool_outcome(&tc.name, &outcome.content, outcome.is_error)
            .await;

        reason_ctx
            .messages
            .push(ChatMessage::tool_result(&tc.id, &tc.name, outcome.content));
    }

    /// Handle rejected tool call outcome.
    pub(super) async fn handle_rejected_tool(
        &self,
        tc: &crate::llm::ToolCall,
        error_msg: &str,
        reason_ctx: &mut ReasoningContext,
    ) {
        {
            let mut sess = self.session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&self.thread_id)
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
                self.fold_into_context(
                    tc,
                    ToolExecutionOutcome {
                        content: error_msg,
                        is_error: true,
                    },
                    reason_ctx,
                )
                .await;
                return None;
            }
        };

        // Detect image generation sentinel
        let is_image_sentinel = self.maybe_emit_image_sentinel(&tc.name, output).await;

        // Always run tool output through the sanitizer/validator/policy/leak-detector pipeline
        let (preview_text, wrapped_text) = self.sanitize_output(&tc.name, output);
        let preview = truncate_for_preview(&preview_text, PREVIEW_MAX_CHARS);
        let result_content = wrapped_text;

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
        self.fold_into_context(
            tc,
            ToolExecutionOutcome {
                content: result_content,
                is_error: is_tool_error,
            },
            reason_ctx,
        )
        .await;

        auth_instructions
    }
}
