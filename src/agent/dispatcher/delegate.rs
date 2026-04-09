//! Chat delegate implementation for the agentic loop.

use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::session::{Session, ThreadState};
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::context::JobContext;
use crate::error::Error;

use crate::agent::agentic_loop::{LoopOutcome, LoopSignal, NativeLoopDelegate, TextAction};
use crate::llm::{ChatMessage, Reasoning, ReasoningContext};
use crate::tools::redact_params;

use super::types::*;

/// Delegate for the chat (dispatcher) context.
///
/// Implements `LoopDelegate` to customize the shared agentic loop for
/// interactive chat sessions with the full 3-phase tool execution
/// (preflight -> parallel exec -> post-flight), approval flow, hooks,
/// auth intercept, and cost tracking.
pub(super) struct ChatDelegate<'a> {
    pub(super) agent: &'a Agent,
    pub(super) session: Arc<Mutex<Session>>,
    pub(super) thread_id: Uuid,
    pub(super) message: &'a IncomingMessage,
    pub(super) job_ctx: JobContext,
    pub(super) active_skills: Vec<crate::skills::LoadedSkill>,
    pub(super) cached_prompt: String,
    pub(super) cached_prompt_no_tools: String,
    pub(super) nudge_at: usize,
    pub(super) force_text_at: usize,
    pub(super) user_tz: chrono_tz::Tz,
}

impl<'a> ChatDelegate<'a> {
    /// Group tool calls into preflight outcomes and runnable batch.
    async fn group_tool_calls(
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
            let hook_params = redact_params(&tc.arguments, sensitive);
            let event = crate::hooks::HookEvent::ToolCall {
                tool_name: tc.name.clone(),
                parameters: hook_params,
                user_id: self.message.user_id.clone(),
                context: "chat".to_string(),
            };
            match self.agent.hooks().run(&event).await {
                Err(crate::hooks::HookError::Rejected { reason }) => {
                    preflight.push((
                        tc,
                        PreflightOutcome::Rejected(format!(
                            "Tool call rejected by hook: {}",
                            reason
                        )),
                    ));
                    continue;
                }
                Err(err) => {
                    preflight.push((
                        tc,
                        PreflightOutcome::Rejected(format!(
                            "Tool call blocked by hook policy: {}",
                            err
                        )),
                    ));
                    continue;
                }
                Ok(crate::hooks::HookOutcome::Continue {
                    modified: Some(new_params),
                }) => match serde_json::from_str::<serde_json::Value>(&new_params) {
                    Ok(mut parsed) => {
                        if let Some(obj) = parsed.as_object_mut() {
                            for key in sensitive {
                                if let Some(orig_val) = original_tc.arguments.get(*key) {
                                    obj.insert((*key).to_string(), orig_val.clone());
                                }
                            }
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
                },
                _ => {}
            }

            // Check if tool requires approval
            if !self.agent.config.auto_approve_tools
                && let Some(tool) = tool_opt
            {
                use crate::tools::ApprovalRequirement;
                let needs_approval = match tool.requires_approval(&tc.arguments) {
                    ApprovalRequirement::Never => false,
                    ApprovalRequirement::UnlessAutoApproved => {
                        let sess = self.session.lock().await;
                        !sess.is_tool_auto_approved(&tc.name)
                    }
                    ApprovalRequirement::Always => true,
                };

                if needs_approval {
                    approval_needed = Some((idx, tc, tool));
                    break;
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
    async fn send_tool_started(&self, tool_name: &str) {
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
    async fn send_tool_completed(
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
    async fn execute_one_tool(&self, tc: &crate::llm::ToolCall) -> Result<String, Error> {
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
    fn sanitize_output(&self, tool_name: &str, output: &str) -> (String, String) {
        let sanitized = self.agent.safety().sanitize_tool_output(tool_name, output);
        let preview_text = sanitized.content.clone();
        let wrapped_text =
            self.agent
                .safety()
                .wrap_for_llm(tool_name, &sanitized.content, sanitized.was_modified);
        (preview_text, wrapped_text)
    }

    /// Record tool outcome in the thread.
    async fn record_tool_outcome(
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
    async fn maybe_emit_image_sentinel(&self, tool_name: &str, output: &str) -> bool {
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
    async fn fold_into_context(
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

    /// Run a batch of tools inline (sequential execution for small batches).
    async fn run_tool_batch_inline(
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
    async fn run_tool_batch_parallel(
        &self,
        runnable: &[(usize, crate::llm::ToolCall)],
        exec_results: &mut [Option<Result<String, Error>>],
    ) {
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
                    &tc.name,
                    &tc.arguments,
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
    async fn handle_rejected_tool(
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
    async fn process_runnable_tool(
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

impl<'a> NativeLoopDelegate for ChatDelegate<'a> {
    async fn check_signals(&self) -> LoopSignal {
        let sess = self.session.lock().await;
        if let Some(thread) = sess.threads.get(&self.thread_id)
            && thread.state == ThreadState::Interrupted
        {
            return LoopSignal::Stop;
        }
        LoopSignal::Continue
    }

    async fn before_llm_call(
        &self,
        reason_ctx: &mut ReasoningContext,
        iteration: usize,
    ) -> Option<LoopOutcome> {
        // Inject a nudge message when approaching the iteration limit so the
        // LLM is aware it should produce a final answer on the next turn.
        if iteration == self.nudge_at {
            reason_ctx.messages.push(ChatMessage::system(
                "You are approaching the tool call limit. \
                 Provide your best final answer on the next response \
                 using the information you have gathered so far. \
                 Do not call any more tools.",
            ));
        }

        let force_text = iteration >= self.force_text_at;

        // Refresh tool definitions each iteration so newly built tools become visible
        let tool_defs = self.agent.tools().tool_definitions().await;

        // Apply trust-based tool attenuation if skills are active.
        let tool_defs = if !self.active_skills.is_empty() {
            let result = crate::skills::attenuate_tools(&tool_defs, &self.active_skills);
            tracing::debug!(
                min_trust = %result.min_trust,
                tools_available = result.tools.len(),
                tools_removed = result.removed_tools.len(),
                removed = ?result.removed_tools,
                explanation = %result.explanation,
                "Tool attenuation applied"
            );
            result.tools
        } else {
            tool_defs
        };

        // Update context for this iteration
        reason_ctx.available_tools = tool_defs;
        reason_ctx.system_prompt = Some(if force_text {
            self.cached_prompt_no_tools.clone()
        } else {
            self.cached_prompt.clone()
        });
        reason_ctx.force_text = force_text;

        if force_text {
            tracing::info!(
                iteration,
                "Forcing text-only response (iteration limit reached)"
            );
        }

        let _ = self
            .agent
            .channels
            .send_status(
                &self.message.channel,
                StatusUpdate::Thinking("Calling LLM...".into()),
                &self.message.metadata,
            )
            .await;

        None
    }

    async fn call_llm(
        &self,
        reasoning: &Reasoning,
        reason_ctx: &mut ReasoningContext,
        iteration: usize,
    ) -> Result<crate::llm::RespondOutput, Error> {
        // Enforce cost guardrails before the LLM call
        if let Err(limit) = self.agent.cost_guard().check_allowed().await {
            return Err(crate::error::LlmError::InvalidResponse {
                provider: "agent".to_string(),
                reason: limit.to_string(),
            }
            .into());
        }

        let output = match reasoning.respond_with_tools(reason_ctx).await {
            Ok(output) => output,
            Err(crate::error::LlmError::ContextLengthExceeded { used, limit }) => {
                tracing::warn!(
                    used,
                    limit,
                    iteration,
                    "Context length exceeded, compacting messages and retrying"
                );

                // Compact messages in place and retry
                reason_ctx.messages = compact_messages_for_retry(&reason_ctx.messages);

                // When force_text, clear tools to further reduce token count
                if reason_ctx.force_text {
                    reason_ctx.available_tools.clear();
                }

                reasoning
                    .respond_with_tools(reason_ctx)
                    .await
                    .map_err(|retry_err| {
                        tracing::error!(
                            original_used = used,
                            original_limit = limit,
                            retry_error = %retry_err,
                            "Retry after auto-compaction also failed"
                        );
                        crate::error::Error::from(retry_err)
                    })?
            }
            Err(e) => return Err(e.into()),
        };

        // Record cost and track token usage
        let model_name = self.agent.llm().active_model_name();
        let read_discount = self.agent.llm().cache_read_discount();
        let write_multiplier = self.agent.llm().cache_write_multiplier();
        let call_cost = self
            .agent
            .cost_guard()
            .record_llm_call(
                &model_name,
                output.usage.input_tokens,
                output.usage.output_tokens,
                output.usage.cache_read_input_tokens,
                output.usage.cache_creation_input_tokens,
                read_discount,
                write_multiplier,
                Some(self.agent.llm().cost_per_token()),
            )
            .await;
        tracing::debug!(
            "LLM call used {} input + {} output tokens (${:.6})",
            output.usage.input_tokens,
            output.usage.output_tokens,
            call_cost,
        );

        Ok(output)
    }

    async fn handle_text_response(
        &self,
        text: &str,
        _reason_ctx: &mut ReasoningContext,
    ) -> TextAction {
        // Strip internal "[Called tool ...]" text that can leak when
        // provider flattening (e.g. NEAR AI) converts tool_calls to
        // plain text and the LLM echoes it back.
        let sanitized = strip_internal_tool_call_text(text);
        TextAction::Return(LoopOutcome::Response(sanitized))
    }

    async fn execute_tool_calls(
        &self,
        tool_calls: Vec<crate::llm::ToolCall>,
        content: Option<String>,
        reason_ctx: &mut ReasoningContext,
    ) -> Result<Option<LoopOutcome>, Error> {
        // Add the assistant message with tool_calls to context.
        // OpenAI protocol requires this before tool-result messages.
        reason_ctx
            .messages
            .push(ChatMessage::assistant_with_tool_calls(
                content,
                tool_calls.clone(),
            ));

        // Execute tools and add results to context
        let _ = self
            .agent
            .channels
            .send_status(
                &self.message.channel,
                StatusUpdate::Thinking(format!("Executing {} tool(s)...", tool_calls.len())),
                &self.message.metadata,
            )
            .await;

        // Record tool calls in the thread with sensitive params redacted.
        {
            let mut redacted_args: Vec<serde_json::Value> = Vec::with_capacity(tool_calls.len());
            for tc in &tool_calls {
                let safe = if let Some(tool) = self.agent.tools().get(&tc.name).await {
                    redact_params(&tc.arguments, tool.sensitive_params())
                } else {
                    tc.arguments.clone()
                };
                redacted_args.push(safe);
            }
            let mut sess = self.session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&self.thread_id)
                && let Some(turn) = thread.last_turn_mut()
            {
                for (tc, safe_args) in tool_calls.iter().zip(redacted_args) {
                    turn.record_tool_call(&tc.name, safe_args);
                }
            }
        }

        // === Phase 1: Preflight (sequential) ===
        let (batch, approval_needed) = self.group_tool_calls(&tool_calls).await?;
        let ToolBatch {
            preflight,
            runnable,
        } = batch;

        // === Phase 2: Parallel execution ===
        let mut exec_results: Vec<Option<Result<String, Error>>> =
            (0..preflight.len()).map(|_| None).collect();

        if runnable.len() <= 1 {
            self.run_tool_batch_inline(&runnable, &mut exec_results)
                .await;
        } else {
            self.run_tool_batch_parallel(&runnable, &mut exec_results)
                .await;
        }

        // === Phase 3: Post-flight (sequential, in original order) ===
        let mut deferred_auth: Option<String> = None;

        for (pf_idx, (tc, outcome)) in preflight.into_iter().enumerate() {
            match outcome {
                PreflightOutcome::Rejected(error_msg) => {
                    self.handle_rejected_tool(&tc, &error_msg, reason_ctx).await;
                }
                PreflightOutcome::Runnable => {
                    let tool_result = exec_results[pf_idx].take().unwrap_or_else(|| {
                        Err(crate::error::ToolError::ExecutionFailed {
                            name: tc.name.clone(),
                            reason: "No result available".to_string(),
                        }
                        .into())
                    });

                    if let Some(instructions) = self
                        .process_runnable_tool(&tc, tool_result, reason_ctx)
                        .await
                    {
                        deferred_auth = Some(instructions);
                    }
                }
            }
        }

        // Return auth response after all results are recorded
        if let Some(instructions) = deferred_auth {
            return Ok(Some(LoopOutcome::Response(instructions)));
        }

        // Handle approval if a tool needed it
        if let Some((approval_idx, tc, tool)) = approval_needed {
            let display_params = redact_params(&tc.arguments, tool.sensitive_params());
            let pending = crate::agent::session::PendingApproval {
                request_id: Uuid::new_v4(),
                tool_name: tc.name.clone(),
                parameters: tc.arguments.clone(),
                display_parameters: display_params,
                description: tool.description().to_string(),
                tool_call_id: tc.id.clone(),
                context_messages: reason_ctx.messages.clone(),
                deferred_tool_calls: tool_calls[approval_idx + 1..].to_vec(),
                user_timezone: Some(self.user_tz.name().to_string()),
            };

            return Ok(Some(LoopOutcome::NeedApproval(Box::new(pending))));
        }

        Ok(None)
    }
}
