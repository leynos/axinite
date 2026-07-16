//! Agentic tool loop for lightweight routines: bounded-iteration LLM/tool
//! cycle with approval gating, validation, timeouts, and output sanitization.

use uuid::Uuid;

use crate::agent::routine::Routine;
use crate::context::JobContext;
use crate::error::RoutineError;
use crate::llm::{ChatMessage, CompletionRequest, ToolCall, ToolCompletionRequest};
use crate::tools::{ApprovalRequirement, ToolError};

use super::execution::{EngineContext, RunOutcome};
use super::lightweight::{PreparedPrompt, handle_text_response, initial_messages};

/// Running token totals accumulated across loop iterations.
#[derive(Default)]
struct TokenTotals {
    input: u32,
    output: u32,
}

impl TokenTotals {
    /// Adds one response's token usage to the totals.
    fn add(&mut self, input: u32, output: u32) {
        self.input += input;
        self.output += output;
    }
}

/// Immutable per-run context shared across tool-loop iterations.
struct ToolLoopContext<'a> {
    ctx: &'a EngineContext,
    job_ctx: &'a JobContext,
    max_tokens: u32,
}

/// Builds a minimal job context for routine tool execution with a unique
/// run ID.
fn routine_job_context(routine: &Routine) -> JobContext {
    JobContext {
        job_id: Uuid::new_v4(),
        user_id: routine.user_id.clone(),
        title: "Lightweight Routine".to_string(),
        description: routine.name.clone(),
        ..Default::default()
    }
}

/// Execute a lightweight routine with tool execution support (agentic loop).
///
/// This is a simplified version of the full dispatcher loop:
/// - Max 3-5 iterations (configurable)
/// - Sequential tool execution (not parallel)
/// - Auto-approval of non-Always tools
/// - No hooks or approval dialogs
pub(super) async fn execute_lightweight_with_tools(
    ctx: &EngineContext,
    routine: &Routine,
    prepared: &PreparedPrompt,
) -> Result<RunOutcome, RoutineError> {
    let mut messages = initial_messages(&prepared.system_prompt, &prepared.full_prompt);
    let max_iterations = ctx.config.lightweight_max_iterations.min(5);
    let mut totals = TokenTotals::default();
    let job_ctx = routine_job_context(routine);
    let loop_ctx = ToolLoopContext {
        ctx,
        job_ctx: &job_ctx,
        max_tokens: prepared.max_tokens,
    };

    // Tool-enabled iterations; the final iteration is reserved for a
    // text-only response.
    for _ in 1..max_iterations {
        let outcome = tool_iteration(&loop_ctx, &mut messages, &mut totals).await?;
        if let Some(result) = outcome {
            return Ok(result);
        }
    }

    final_text_iteration(ctx, messages, prepared.max_tokens, &totals).await
}

/// Runs the forced text-only final iteration (no tools offered).
async fn final_text_iteration(
    ctx: &EngineContext,
    messages: Vec<ChatMessage>,
    effective_max_tokens: u32,
    totals: &TokenTotals,
) -> Result<RunOutcome, RoutineError> {
    let request = CompletionRequest::new(messages)
        .with_max_tokens(effective_max_tokens)
        .with_temperature(0.3);

    let response = ctx
        .llm
        .complete(request)
        .await
        .map_err(RoutineError::from)?;

    handle_text_response(
        &response.content,
        response.finish_reason,
        totals.input + response.input_tokens,
        totals.output + response.output_tokens,
    )
}

/// One tool-enabled iteration of the agentic loop.
///
/// Returns `Some(result)` when the LLM answered with text (the loop is
/// finished), or `None` after executing the requested tool calls (the loop
/// should continue).
async fn tool_iteration(
    loop_ctx: &ToolLoopContext<'_>,
    messages: &mut Vec<ChatMessage>,
    totals: &mut TokenTotals,
) -> Result<Option<RunOutcome>, RoutineError> {
    let ctx = loop_ctx.ctx;
    let job_ctx = loop_ctx.job_ctx;
    let tool_defs = ctx.tools.tool_definitions().await;

    let request = ToolCompletionRequest::new(messages.clone(), tool_defs)
        .with_max_tokens(loop_ctx.max_tokens)
        .with_temperature(0.3);

    let response = ctx
        .llm
        .complete_with_tools(request)
        .await
        .map_err(RoutineError::from)?;

    totals.add(response.input_tokens, response.output_tokens);

    // Check if LLM returned text (no tool calls)
    if response.tool_calls.is_empty() {
        let content = response.content.unwrap_or_default();
        return handle_text_response(
            &content,
            response.finish_reason,
            totals.input,
            totals.output,
        )
        .map(Some);
    }

    // LLM returned tool calls: add assistant message and execute tools
    messages.push(ChatMessage::assistant_with_tool_calls(
        response.content.clone(),
        response.tool_calls.clone(),
    ));

    // Execute tools sequentially
    for tc in response.tool_calls {
        let result = execute_routine_tool(ctx, job_ctx, &tc).await;
        let result_content = sanitize_tool_result(ctx, &tc, result);
        // Add tool result to context
        messages.push(ChatMessage::tool_result(&tc.id, &tc.name, &result_content));
    }

    // Continue loop to next LLM call
    Ok(None)
}

/// Sanitizes and wraps a tool result (or its error) for LLM consumption.
fn sanitize_tool_result(
    ctx: &EngineContext,
    tc: &ToolCall,
    result: Result<String, Box<dyn std::error::Error + Send + Sync>>,
) -> String {
    let raw = match result {
        Ok(output) => output,
        Err(e) => format!("Tool '{}' failed: {}", tc.name, e),
    };
    let sanitized = ctx.safety.sanitize_tool_output(&tc.name, &raw);
    ctx.safety
        .wrap_for_llm(&tc.name, &sanitized.content, sanitized.was_modified)
}

/// Execute a single tool for a lightweight routine.
async fn execute_routine_tool(
    ctx: &EngineContext,
    job_ctx: &JobContext,
    tc: &ToolCall,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // Check if tool exists
    let tool = ctx
        .tools
        .get(&tc.name)
        .await
        .ok_or_else(|| format!("Tool '{}' not found", tc.name))?;

    // Check approval requirement: only allow Never tools in lightweight routines.
    // UnlessAutoApproved and Always tools are blocked to prevent prompt injection attacks.
    // Lightweight routines can be triggered by external events and may process untrusted data,
    // making them vulnerable to prompt injection that could trick the LLM into calling
    // sensitive tools. Blocking these tools entirely is the safest approach.
    match tool.requires_approval(&tc.arguments) {
        ApprovalRequirement::Never => {}
        ApprovalRequirement::UnlessAutoApproved | ApprovalRequirement::Always => {
            return Err(format!(
                "Tool '{}' requires manual approval and cannot be used in lightweight routines",
                tc.name
            )
            .into());
        }
    }

    // Validate tool parameters
    let validation = ctx.safety.validator().validate_tool_params(&tc.arguments);
    if !validation.is_valid {
        let details = validation
            .errors
            .iter()
            .map(|e| format!("{}: {}", e.field, e.message))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!("Invalid tool parameters: {}", details).into());
    }

    // Execute with per-tool timeout
    let timeout = tool.execution_timeout();
    let start = std::time::Instant::now();
    let result = tokio::time::timeout(timeout, async {
        tool.execute(tc.arguments.clone(), job_ctx).await
    })
    .await;
    log_tool_execution(tc, &result, start.elapsed(), timeout);

    let result = result
        .map_err(|_| ToolError::Timeout(timeout))
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

    // Serialize result to JSON string
    let result_str =
        serde_json::to_string(&result.result).unwrap_or_else(|_| "<serialize error>".to_string());
    Ok(result_str)
}

/// Emits the single consolidated log line for a tool execution outcome.
fn log_tool_execution(
    tc: &ToolCall,
    result: &Result<Result<crate::tools::ToolOutput, ToolError>, tokio::time::error::Elapsed>,
    elapsed: std::time::Duration,
    timeout: std::time::Duration,
) {
    match result {
        Ok(Ok(_)) => {
            tracing::debug!(
                tool = %tc.name,
                elapsed_ms = elapsed.as_millis() as u64,
                status = "succeeded",
                "Lightweight routine tool execution completed"
            );
        }
        Ok(Err(e)) => {
            tracing::debug!(
                tool = %tc.name,
                elapsed_ms = elapsed.as_millis() as u64,
                error = %e,
                status = "failed",
                "Lightweight routine tool execution completed"
            );
        }
        Err(_) => {
            tracing::debug!(
                tool = %tc.name,
                elapsed_ms = elapsed.as_millis() as u64,
                timeout_secs = timeout.as_secs(),
                status = "timeout",
                "Lightweight routine tool execution completed"
            );
        }
    }
}
