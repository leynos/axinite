//! NativeLoopDelegate implementation for ChatDelegate.

use crate::agent::agentic_loop::{LoopOutcome, LoopSignal, NativeLoopDelegate, TextAction};
use crate::agent::session::ThreadState;
use crate::channels::StatusUpdate;
use crate::error::Error;
use crate::llm::{ChatMessage, Reasoning, ReasoningContext};
use crate::tools::redact_params;
use uuid::Uuid;

use super::ChatDelegate;
use crate::agent::dispatcher::types::*;

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

                let retry_result: Result<crate::llm::RespondOutput, crate::error::LlmError> =
                    reasoning.respond_with_tools(reason_ctx).await;
                retry_result.map_err(|retry_err| {
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
