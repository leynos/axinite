//! Tool execution logic for the chat delegate.
//!
//! Contains the execute_tool_calls implementation and all helper methods
//! for the 3-phase tool execution pipeline (preflight → execution → post-flight).

use std::sync::Arc;

use tokio::task::JoinSet;
use uuid::Uuid;

use crate::agent::dispatcher::delegate::ChatDelegate;
use crate::agent::session::PendingApproval;
use crate::channels::StatusUpdate;
use crate::context::JobContext;
use crate::error::Error;
use crate::llm::{ChatMessage, ReasoningContext};
use crate::safety::SafetyLayer;
use crate::tools::{ToolRegistry, redact_params};

/// Outcome of preflight check for a single tool call.
pub(crate) enum PreflightOutcome {
    /// Tool call was rejected by a hook.
    Rejected(String),
    /// Tool call is runnable.
    Runnable,
}

/// Result of grouping tool calls into batches.
pub(crate) struct ToolBatch {
    /// Preflight outcomes for each tool call.
    pub(super) preflight: Vec<(crate::llm::ToolCall, PreflightOutcome)>,
    /// Indices of runnable tools (pointing into preflight).
    pub(super) runnable: Vec<(usize, crate::llm::ToolCall)>,
}

/// A tool call that requires user approval, together with its index in the
/// original call sequence (used to build the deferred-call slice).
pub(super) struct ApprovalCandidate {
    pub idx: usize,
    pub tool_call: crate::llm::ToolCall,
    pub tool: Arc<dyn crate::tools::Tool>,
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

/// Allocate the exec-results buffer and dispatch Phase 2 tool execution.
async fn run_phase2(
    delegate: &ChatDelegate<'_>,
    preflight_len: usize,
    runnable: &[(usize, crate::llm::ToolCall)],
) -> Vec<Option<Result<String, Error>>> {
    let mut exec_results: Vec<Option<Result<String, Error>>> =
        (0..preflight_len).map(|_| None).collect();
    if runnable.len() <= 1 {
        run_tool_batch_inline(delegate, runnable, &mut exec_results).await;
    } else {
        run_tool_batch_parallel(delegate, runnable, &mut exec_results).await;
    }
    exec_results
}

/// Phase 3: iterate preflight outcomes in original order, dispatching each
/// to `handle_rejected_tool` or `process_runnable_tool`.
/// Returns the first deferred-auth instruction string, if any.
async fn run_postflight(
    delegate: &ChatDelegate<'_>,
    preflight: Vec<(crate::llm::ToolCall, PreflightOutcome)>,
    exec_results: &mut [Option<Result<String, Error>>],
    reason_ctx: &mut ReasoningContext,
) -> Option<String> {
    let mut deferred_auth: Option<String> = None;
    for (pf_idx, (tc, outcome)) in preflight.into_iter().enumerate() {
        match outcome {
            PreflightOutcome::Rejected(error_msg) => {
                handle_rejected_tool(delegate, &tc, &error_msg, reason_ctx).await;
            }
            PreflightOutcome::Runnable => {
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
                }
            }
        }
    }
    deferred_auth
}

/// Construct the `PendingApproval` value for a tool that requires user consent.
fn build_pending_approval(
    delegate: &ChatDelegate<'_>,
    candidate: ApprovalCandidate,
    tool_calls: &[crate::llm::ToolCall],
    reason_ctx: &ReasoningContext,
) -> PendingApproval {
    let display_params = redact_params(
        &candidate.tool_call.arguments,
        candidate.tool.sensitive_params(),
    );
    PendingApproval {
        request_id: Uuid::new_v4(),
        tool_name: candidate.tool_call.name.clone(),
        parameters: candidate.tool_call.arguments.clone(),
        display_parameters: display_params,
        description: candidate.tool.description().to_string(),
        tool_call_id: candidate.tool_call.id.clone(),
        context_messages: reason_ctx.messages.clone(),
        deferred_tool_calls: tool_calls[candidate.idx + 1..].to_vec(),
        user_timezone: Some(delegate.user_tz.name().to_string()),
    }
}

/// Execute tool calls with 3-phase pipeline (preflight → execution → post-flight).
pub(crate) async fn execute_tool_calls(
    delegate: &ChatDelegate<'_>,
    tool_calls: Vec<crate::llm::ToolCall>,
    content: Option<String>,
    reason_ctx: &mut ReasoningContext,
) -> Result<Option<crate::agent::agentic_loop::LoopOutcome>, Error> {
    use crate::agent::agentic_loop::LoopOutcome;

    // Add the assistant message with tool_calls to context.
    // OpenAI protocol requires this before tool-result messages.
    reason_ctx
        .messages
        .push(ChatMessage::assistant_with_tool_calls(
            content,
            tool_calls.clone(),
        ));

    let _ = delegate
        .agent
        .channels
        .send_status(
            &delegate.message.channel,
            StatusUpdate::Thinking(format!("Executing {} tool(s)...", tool_calls.len())),
            &delegate.message.metadata,
        )
        .await;

    record_redacted_tool_calls(delegate, &tool_calls).await;

    // === Phase 1: Preflight ===
    let (batch, approval_needed) = group_tool_calls(delegate, &tool_calls).await?;
    let ToolBatch {
        preflight,
        runnable,
    } = batch;

    // === Phase 2: Execute ===
    let mut exec_results = run_phase2(delegate, preflight.len(), &runnable).await;

    // === Phase 3: Post-flight ===
    let deferred_auth = run_postflight(delegate, preflight, &mut exec_results, reason_ctx).await;

    if let Some(candidate) = approval_needed {
        let pending = build_pending_approval(delegate, candidate, &tool_calls, reason_ctx);
        return Ok(Some(LoopOutcome::NeedApproval(Box::new(pending))));
    }

    if let Some(instructions) = deferred_auth {
        return Ok(Some(LoopOutcome::Response(instructions)));
    }

    Ok(None)
}

/// Compute the safe (redacted) argument map for a single tool call.
async fn redact_single_tool_call(
    agent: &crate::agent::Agent,
    tc: &crate::llm::ToolCall,
) -> serde_json::Value {
    if let Some(tool) = agent.tools().get(&tc.name).await {
        redact_params(&tc.arguments, tool.sensitive_params())
    } else {
        tc.arguments.clone()
    }
}

/// Record redacted tool-call args into the current turn of the session thread.
async fn write_tool_calls_to_thread(
    delegate: &ChatDelegate<'_>,
    tool_calls: &[crate::llm::ToolCall],
    redacted_args: Vec<serde_json::Value>,
) {
    let mut sess = delegate.session.lock().await;
    let Some(thread) = sess.threads.get_mut(&delegate.thread_id) else {
        return;
    };
    let Some(turn) = thread.last_turn_mut() else {
        return;
    };
    for (tc, safe_args) in tool_calls.iter().zip(redacted_args) {
        turn.record_tool_call(&tc.name, safe_args);
    }
}

/// Record tool calls in the session thread with sensitive params redacted.
async fn record_redacted_tool_calls(
    delegate: &ChatDelegate<'_>,
    tool_calls: &[crate::llm::ToolCall],
) {
    let mut redacted_args: Vec<serde_json::Value> = Vec::with_capacity(tool_calls.len());
    for tc in tool_calls {
        redacted_args.push(redact_single_tool_call(delegate.agent, tc).await);
    }
    write_tool_calls_to_thread(delegate, tool_calls, redacted_args).await;
}

/// Restore original values for sensitive fields into a mutable JSON object.
///
/// After a hook modifies tool parameters, any sensitive key that was
/// redacted before the hook must be put back from the original call to
/// prevent secret loss.
fn restore_sensitive_fields(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    original_args: &serde_json::Value,
    sensitive: &[&str],
) {
    for key in sensitive {
        if let Some(orig_val) = original_args.get(*key) {
            obj.insert((*key).to_string(), orig_val.clone());
        }
    }
}

/// Apply hook parameter modification to a tool call.
fn apply_hook_param_modification(
    tc: &mut crate::llm::ToolCall,
    original_tc: &crate::llm::ToolCall,
    sensitive: &[&str],
    new_params: &str,
) {
    match serde_json::from_str::<serde_json::Value>(new_params) {
        Ok(mut parsed) => {
            if let Some(obj) = parsed.as_object_mut() {
                restore_sensitive_fields(obj, &original_tc.arguments, sensitive);
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

/// Apply the BeforeToolCall hook and return rejection message if any.
async fn apply_before_tool_call_hook(
    delegate: &ChatDelegate<'_>,
    original_tc: &crate::llm::ToolCall,
    tc: &mut crate::llm::ToolCall,
    sensitive: &[&str],
) -> Option<String> {
    let hook_params = redact_params(&tc.arguments, sensitive);
    let event = crate::hooks::HookEvent::ToolCall {
        tool_name: tc.name.clone(),
        parameters: hook_params,
        user_id: delegate.message.user_id.clone(),
        context: "chat".to_string(),
    };
    match delegate.agent.hooks().run(&event).await {
        Err(crate::hooks::HookError::Rejected { reason }) => {
            Some(format!("Tool call rejected by hook: {}", reason))
        }
        Err(err) => Some(format!("Tool call blocked by hook policy: {}", err)),
        Ok(crate::hooks::HookOutcome::Continue {
            modified: Some(new_params),
        }) => {
            apply_hook_param_modification(tc, original_tc, sensitive, &new_params);
            None
        }
        _ => None,
    }
}

/// Check if a tool requires approval based on its configuration and auto-approve settings.
async fn tool_requires_approval(
    delegate: &ChatDelegate<'_>,
    tool: &std::sync::Arc<dyn crate::tools::Tool>,
    tc: &crate::llm::ToolCall,
) -> bool {
    use crate::tools::ApprovalRequirement;
    match tool.requires_approval(&tc.arguments) {
        ApprovalRequirement::Never => false,
        ApprovalRequirement::Always => true,
        ApprovalRequirement::UnlessAutoApproved => {
            let sess = delegate.session.lock().await;
            !sess.is_tool_auto_approved(&tc.name)
        }
    }
}

/// Group tool calls into preflight outcomes and runnable batch.
async fn group_tool_calls(
    delegate: &ChatDelegate<'_>,
    tool_calls: &[crate::llm::ToolCall],
) -> Result<(ToolBatch, Option<ApprovalCandidate>), Error> {
    let mut preflight: Vec<(crate::llm::ToolCall, PreflightOutcome)> = Vec::new();
    let mut runnable: Vec<(usize, crate::llm::ToolCall)> = Vec::new();
    let mut approval_needed = None;

    for (idx, original_tc) in tool_calls.iter().enumerate() {
        let mut tc = original_tc.clone();

        let tool_opt = delegate.agent.tools().get(&tc.name).await;
        let sensitive = tool_opt
            .as_ref()
            .map(|t| t.sensitive_params())
            .unwrap_or(&[]);

        // Hook: BeforeToolCall
        if let Some(rejection_msg) =
            apply_before_tool_call_hook(delegate, original_tc, &mut tc, sensitive).await
        {
            preflight.push((tc, PreflightOutcome::Rejected(rejection_msg)));
            continue;
        }

        // Check if tool requires approval
        if !delegate.agent.config.auto_approve_tools
            && let Some(tool) = tool_opt
        {
            if tool_requires_approval(delegate, &tool, &tc).await {
                approval_needed = Some(ApprovalCandidate {
                    idx,
                    tool_call: tc,
                    tool,
                });
                break;
            }
            let preflight_idx = preflight.len();
            preflight.push((tc.clone(), PreflightOutcome::Runnable));
            runnable.push((preflight_idx, tc));
            continue;
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

/// Run a batch of tools inline (sequential execution for small batches).
async fn run_tool_batch_inline(
    delegate: &ChatDelegate<'_>,
    runnable: &[(usize, crate::llm::ToolCall)],
    exec_results: &mut [Option<Result<String, Error>>],
) {
    for (pf_idx, tc) in runnable {
        let result = execute_one_tool(delegate, tc).await;
        exec_results[*pf_idx] = Some(result);
    }
}

/// Run a batch of tools in parallel (for large batches).
async fn run_tool_batch_parallel(
    delegate: &ChatDelegate<'_>,
    runnable: &[(usize, crate::llm::ToolCall)],
    exec_results: &mut [Option<Result<String, Error>>],
) {
    let mut join_set = JoinSet::new();

    for (pf_idx, tc) in runnable {
        let pf_idx = *pf_idx;
        let tools = delegate.agent.tools().clone();
        let safety = delegate.agent.safety().clone();
        let channels = delegate.agent.channels.clone();
        let job_ctx = delegate.job_ctx.clone();
        let tc = tc.clone();
        let channel = delegate.message.channel.clone();
        let metadata = delegate.message.metadata.clone();

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
                &ToolCallSpec {
                    name: &tc.name,
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

/// Execute a single tool inline (for small batches).
async fn execute_one_tool(
    delegate: &ChatDelegate<'_>,
    tc: &crate::llm::ToolCall,
) -> Result<String, Error> {
    send_tool_started(delegate, &tc.name).await;
    let result = delegate
        .agent
        .execute_chat_tool(&tc.name, &tc.arguments, &delegate.job_ctx)
        .await;
    send_tool_completed(delegate, &tc.name, &result, &tc.arguments).await;
    result
}

/// Send ToolStarted status update.
async fn send_tool_started(delegate: &ChatDelegate<'_>, tool_name: &str) {
    let _ = delegate
        .agent
        .channels
        .send_status(
            &delegate.message.channel,
            StatusUpdate::ToolStarted {
                name: tool_name.to_string(),
            },
            &delegate.message.metadata,
        )
        .await;
}

/// Send tool_completed status update.
async fn send_tool_completed(
    delegate: &ChatDelegate<'_>,
    tool_name: &str,
    result: &Result<String, Error>,
    arguments: &serde_json::Value,
) {
    let disp_tool = delegate.agent.tools().get(tool_name).await;
    let _ = delegate
        .agent
        .channels
        .send_status(
            &delegate.message.channel,
            StatusUpdate::tool_completed(
                tool_name.to_string(),
                result,
                arguments,
                disp_tool.as_deref(),
            ),
            &delegate.message.metadata,
        )
        .await;
}

/// Handle rejected tool call outcome.
async fn handle_rejected_tool(
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
async fn process_runnable_tool(
    delegate: &ChatDelegate<'_>,
    tc: &crate::llm::ToolCall,
    tool_result: Result<String, Error>,
    reason_ctx: &mut ReasoningContext,
) -> Option<String> {
    use crate::agent::dispatcher::{PREVIEW_MAX_CHARS, is_valid_json, truncate_for_preview};

    let is_tool_error = tool_result.is_err();

    // Handle error case early
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

    // Detect image generation sentinel
    let is_image_sentinel = maybe_emit_image_sentinel(delegate, &tc.name, output).await;

    // Determine result content and preview based on whether output is valid JSON
    let (result_content, preview) = if is_valid_json(output) {
        // For JSON-producing tools, persist raw JSON without wrapping
        let preview = truncate_for_preview(output, PREVIEW_MAX_CHARS);
        (output.clone(), preview)
    } else {
        // Sanitize tool output first (before sending preview or using in context)
        // preview_text is raw sanitized for preview, wrapped_text is for LLM context
        let (preview_text, wrapped_text) = sanitize_output(delegate, &tc.name, output);
        let preview = truncate_for_preview(&preview_text, PREVIEW_MAX_CHARS);
        (wrapped_text, preview)
    };

    // Send ToolResult preview
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

    // Check for auth awaiting (use original tool_result for auth detection)
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

    // Stash full output so subsequent tools can reference it
    delegate
        .job_ctx
        .tool_output_stash
        .write()
        .await
        .insert(tc.id.clone(), output.clone());

    // Fold result into context
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
struct ToolOutcome {
    result_content: String,
    is_tool_error: bool,
}

/// Fold tool result into context messages.
async fn fold_into_context(
    delegate: &ChatDelegate<'_>,
    tc: &crate::llm::ToolCall,
    outcome: ToolOutcome,
    reason_ctx: &mut ReasoningContext,
) {
    // Record sanitized result in thread
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

/// Record tool outcome in the thread.
async fn record_tool_outcome(
    delegate: &ChatDelegate<'_>,
    _tool_name: &str,
    result_content: &str,
    is_tool_error: bool,
) {
    let mut sess = delegate.session.lock().await;
    if let Some(thread) = sess.threads.get_mut(&delegate.thread_id)
        && let Some(turn) = thread.last_turn_mut()
    {
        if is_tool_error {
            turn.record_tool_error(result_content.to_string());
        } else {
            turn.record_tool_result_content(result_content);
        }
    }
}

/// Specification for a tool call to be executed.
pub(crate) struct ToolCallSpec<'a> {
    pub(crate) name: &'a str,
    pub(crate) params: &'a serde_json::Value,
}

/// Execute a chat tool without requiring `&Agent`.
///
/// This standalone function enables parallel invocation from spawned JoinSet
/// tasks, which cannot borrow `&self`. Delegates to the shared
/// `execute_tool_with_safety` pipeline.
pub(crate) async fn execute_chat_tool_standalone(
    tools: &ToolRegistry,
    safety: &SafetyLayer,
    spec: &ToolCallSpec<'_>,
    job_ctx: &JobContext,
) -> Result<String, Error> {
    crate::tools::execute::execute_tool_with_safety(tools, safety, spec.name, spec.params, job_ctx)
        .await
}
