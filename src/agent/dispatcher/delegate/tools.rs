//! Tool execution helpers for ChatDelegate.

use std::sync::Arc;

use crate::channels::StatusUpdate;
use crate::error::Error;
use crate::llm::{ChatMessage, ReasoningContext};
use crate::tools::redact_params;

use super::ChatDelegate;
use crate::agent::dispatcher::types::*;

/// Restore original values of sensitive parameters into a hook-modified JSON
/// object, ensuring that fields the hook was not permitted to see are not
/// inadvertently erased.
fn restore_sensitive_params(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    original_tc: &crate::llm::ToolCall,
    sensitive: &[&str],
) {
    for key in sensitive {
        if let Some(orig_val) = original_tc.arguments.get(*key) {
            obj.insert((*key).to_string(), orig_val.clone());
        }
    }
}

/// Apply hook-modified parameters back onto `tc`, restoring any sensitive
/// fields from the original arguments to prevent them being erased.
fn apply_hook_params(
    tc: &mut crate::llm::ToolCall,
    original_tc: &crate::llm::ToolCall,
    sensitive: &[&str],
    new_params: &str,
) {
    match serde_json::from_str::<serde_json::Value>(new_params) {
        Ok(mut parsed) => {
            if let Some(obj) = parsed.as_object_mut() {
                restore_sensitive_params(obj, original_tc, sensitive);
            }
            tc.arguments = parsed;
        }
        Err(e) => {
            tracing::warn!(
                tool = %tc.name,
                "Hook returned non-JSON modification for ToolCall, ignoring: {}",
                e
            );
        }
    }
}

impl<'a> ChatDelegate<'a> {
    /// Return `true` if tool approval is enforced (auto-approve is disabled).
    fn tool_approval_enforced(&self) -> bool {
        !self.agent.config.auto_approve_tools
    }

    /// Return `true` if `tool` requires human approval for this invocation.
    /// Consults the session's auto-approve list when the requirement is
    /// `UnlessAutoApproved`.
    async fn resolve_needs_approval(
        &self,
        tool: &Arc<dyn crate::tools::Tool>,
        tc_name: &str,
        arguments: &serde_json::Value,
    ) -> bool {
        use crate::tools::ApprovalRequirement;
        match tool.requires_approval(arguments) {
            ApprovalRequirement::Never => false,
            ApprovalRequirement::UnlessAutoApproved => {
                let sess = self.session.lock().await;
                !sess.is_tool_auto_approved(tc_name)
            }
            ApprovalRequirement::Always => true,
        }
    }

    /// Run the `BeforeToolCall` hook for one tool invocation.
    ///
    /// Returns `Some(PreflightOutcome::Rejected(…))` when the hook blocks the
    /// call (the caller should push that outcome and `continue` to the next
    /// tool). Returns `None` when the call should proceed; `tc.arguments` may
    /// have been mutated to incorporate hook-supplied parameter overrides.
    async fn run_tool_hook_preflight(
        &self,
        tc: &mut crate::llm::ToolCall,
        original_tc: &crate::llm::ToolCall,
        sensitive: &[&str],
    ) -> Option<PreflightOutcome> {
        let hook_params = redact_params(&tc.arguments, sensitive);
        let event = crate::hooks::HookEvent::ToolCall {
            tool_name: tc.name.clone(),
            parameters: hook_params,
            user_id: self.message.user_id.clone(),
            context: "chat".to_string(),
        };

        match self.agent.hooks().run(&event).await {
            Err(crate::hooks::HookError::Rejected { reason }) => Some(PreflightOutcome::Rejected(
                format!("Tool call rejected by hook: {}", reason),
            )),
            Err(err) => Some(PreflightOutcome::Rejected(format!(
                "Tool call blocked by hook policy: {}",
                err
            ))),
            Ok(crate::hooks::HookOutcome::Continue {
                modified: Some(new_params),
            }) => {
                apply_hook_params(tc, original_tc, sensitive, &new_params);
                None
            }
            _ => None,
        }
    }

    /// Group tool calls into preflight outcomes and runnable batch.
    // Intentionally decomposed from a complex single-line conditional to reduce
    // cognitive complexity (CodeScene: Complex Conditional).
    #[allow(clippy::collapsible_if)]
    pub(super) async fn group_tool_calls(
        &self,
        tool_calls: &[crate::llm::ToolCall],
    ) -> Result<
        (
            ToolBatch,
            Option<(usize, crate::llm::ToolCall, Arc<dyn crate::tools::Tool>)>,
        ),
        Error,
    > {
        let mut preflight: Vec<(crate::llm::ToolCall, PreflightOutcome)> = Vec::new();
        let mut runnable: Vec<(usize, crate::llm::ToolCall)> = Vec::new();
        let mut approval_needed: Option<(
            usize,
            crate::llm::ToolCall,
            Arc<dyn crate::tools::Tool>,
        )> = None;

        for (idx, original_tc) in tool_calls.iter().enumerate() {
            let mut tc = original_tc.clone();

            let tool_opt = self.agent.tools().get(&tc.name).await;
            let sensitive = tool_opt
                .as_ref()
                .map(|t| t.sensitive_params())
                .unwrap_or(&[]);

            // Hook: BeforeToolCall
            if let Some(rejected) = self
                .run_tool_hook_preflight(&mut tc, original_tc, sensitive)
                .await
            {
                preflight.push((tc, rejected));
                continue;
            }

            // Check if tool requires approval
            if self.tool_approval_enforced() {
                if let Some(tool) = tool_opt {
                    if self
                        .resolve_needs_approval(&tool, &tc.name, &tc.arguments)
                        .await
                    {
                        approval_needed = Some((idx, tc, tool));
                        break;
                    }
                }
            }

            let preflight_idx = preflight.len();
            preflight.push((tc.clone(), PreflightOutcome::Runnable));
            runnable.push((preflight_idx, tc));
        }

        Ok((
            ToolBatch {
                preflight,
                runnable,
            },
            approval_needed,
        ))
    }

    /// Send ToolStarted status update.
    pub(super) async fn send_tool_started(&self, tool_name: &str) {
        let _ = self
            .agent
            .channels
            .send_status(
                &self.message.channel,
                StatusUpdate::ToolStarted {
                    name: tool_name.to_string(),
                },
                &self.message.metadata,
            )
            .await;
    }

    /// Send tool_completed status update.
    pub(super) async fn send_tool_completed(
        &self,
        tool_name: &str,
        result: &Result<String, Error>,
        arguments: &serde_json::Value,
    ) {
        let disp_tool = self.agent.tools().get(tool_name).await;
        let _ = self
            .agent
            .channels
            .send_status(
                &self.message.channel,
                StatusUpdate::tool_completed(
                    tool_name.to_string(),
                    result,
                    arguments,
                    disp_tool.as_deref(),
                ),
                &self.message.metadata,
            )
            .await;
    }

    /// Execute a single tool inline (for small batches).
    pub(super) async fn execute_one_tool(
        &self,
        tc: &crate::llm::ToolCall,
    ) -> Result<String, Error> {
        self.send_tool_started(&tc.name).await;
        let result = self
            .agent
            .execute_chat_tool(&tc.name, &tc.arguments, &self.job_ctx)
            .await;
        self.send_tool_completed(&tc.name, &result, &tc.arguments)
            .await;
        result
    }

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
        outcome: ToolExecutionOutcome,
        reason_ctx: &mut ReasoningContext,
    ) {
        self.record_tool_outcome(&tc.name, &outcome.content, outcome.is_error)
            .await;

        reason_ctx
            .messages
            .push(ChatMessage::tool_result(&tc.id, &tc.name, outcome.content));
    }

    /// Run a batch of tools inline (sequential execution for small batches).
    pub(super) async fn run_tool_batch_inline(
        &self,
        runnable: &[(usize, crate::llm::ToolCall)],
        exec_results: &mut [Option<Result<String, Error>>],
    ) {
        for (pf_idx, tc) in runnable {
            let result = self.execute_one_tool(tc).await;
            exec_results[*pf_idx] = Some(result);
        }
    }

    /// Run a batch of tools in parallel (for large batches).
    pub(super) async fn run_tool_batch_parallel(
        &self,
        runnable: &[(usize, crate::llm::ToolCall)],
        exec_results: &mut [Option<Result<String, Error>>],
    ) {
        use tokio::task::JoinSet;

        let mut join_set = JoinSet::new();

        for (pf_idx, tc) in runnable {
            let pf_idx = *pf_idx;
            let tools = self.agent.tools().clone();
            let safety = self.agent.safety().clone();
            let channels = self.agent.channels.clone();
            let job_ctx = self.job_ctx.clone();
            let tc = tc.clone();
            let channel = self.message.channel.clone();
            let metadata = self.message.metadata.clone();

            join_set.spawn(async move {
                let _ = channels
                    .send_status(
                        &channel,
                        StatusUpdate::ToolStarted {
                            name: tc.name.clone(),
                        },
                        &metadata,
                    )
                    .await;

                let result = execute_chat_tool_standalone(
                    &tools,
                    &safety,
                    &ChatToolRequest {
                        tool_name: &tc.name,
                        params: &tc.arguments,
                    },
                    &job_ctx,
                )
                .await;

                let par_tool = tools.get(&tc.name).await;
                let _ = channels
                    .send_status(
                        &channel,
                        StatusUpdate::tool_completed(
                            tc.name.clone(),
                            &result,
                            &tc.arguments,
                            par_tool.as_deref(),
                        ),
                        &metadata,
                    )
                    .await;

                (pf_idx, result)
            });
        }

        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok((pf_idx, result)) => {
                    exec_results[pf_idx] = Some(result);
                }
                Err(e) => {
                    if e.is_panic() {
                        tracing::error!("Chat tool execution task panicked: {}", e);
                    } else {
                        tracing::error!("Chat tool execution task cancelled: {}", e);
                    }
                }
            }
        }

        // Fill panicked slots with error results
        for (pf_idx, tc) in runnable.iter() {
            if exec_results[*pf_idx].is_none() {
                tracing::error!(
                    tool = %tc.name,
                    "Filling failed task slot with error"
                );
                exec_results[*pf_idx] = Some(Err(crate::error::ToolError::ExecutionFailed {
                    name: tc.name.clone(),
                    reason: "Task failed during execution".to_string(),
                }
                .into()));
            }
        }
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
