//! Lightweight routine execution: prompt assembly from workspace context and
//! state, single-call (no-tools) execution, and text response handling.

use crate::agent::routine::{Routine, RunStatus};
use crate::error::RoutineError;
use crate::llm::{ChatMessage, CompletionRequest, FinishReason};

use super::execution::EngineContext;
use super::lightweight_tools::execute_lightweight_with_tools;

/// Return `true` when a character is safe to appear in a workspace path.
fn is_workspace_safe_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_')
}

/// Sanitize a routine name for use in workspace paths.
/// Only keeps alphanumeric, dash, and underscore characters; replaces everything else.
pub(super) fn sanitize_routine_name(name: &str) -> String {
    name.chars()
        .map(|c| if is_workspace_safe_char(c) { c } else { '_' })
        .collect()
}

/// Execute a lightweight routine with optional tool support.
///
/// If tools are enabled, this runs a simplified agentic loop (max 3-5 iterations).
/// If tools are disabled, this does a single LLM call (original behavior).
pub(super) async fn execute_lightweight(
    ctx: &EngineContext,
    routine: &Routine,
    prompt: &str,
    context_paths: &[String],
    max_tokens: u32,
) -> Result<(RunStatus, Option<String>, Option<i32>), RoutineError> {
    let context_parts = load_context_parts(ctx, routine, context_paths).await;
    let state_content = load_routine_state(ctx, routine).await;
    let full_prompt = build_lightweight_prompt(prompt, &context_parts, state_content.as_deref());
    let system_prompt = load_system_prompt(ctx, routine).await;
    let effective_max_tokens = resolve_max_tokens(ctx, max_tokens).await;

    // If tools are enabled, use the tool execution loop; otherwise, single LLM call
    if ctx.config.lightweight_tools_enabled {
        execute_lightweight_with_tools(
            ctx,
            routine,
            &system_prompt,
            &full_prompt,
            effective_max_tokens,
        )
        .await
    } else {
        execute_lightweight_no_tools(
            ctx,
            routine,
            &system_prompt,
            &full_prompt,
            effective_max_tokens,
        )
        .await
    }
}

/// Reads each context path from the workspace, skipping (and logging)
/// unreadable paths.
async fn load_context_parts(
    ctx: &EngineContext,
    routine: &Routine,
    context_paths: &[String],
) -> Vec<String> {
    let mut context_parts = Vec::new();
    for path in context_paths {
        match ctx.workspace.read(path).await {
            Ok(doc) => {
                context_parts.push(format!("## {}\n\n{}", path, doc.content));
            }
            Err(e) => {
                tracing::debug!(
                    routine = %routine.name,
                    "Failed to read context path {}: {}", path, e
                );
            }
        }
    }
    context_parts
}

/// Loads the routine's persisted state from the workspace, if present.
///
/// The routine name is sanitized to prevent path traversal.
async fn load_routine_state(ctx: &EngineContext, routine: &Routine) -> Option<String> {
    let safe_name = sanitize_routine_name(&routine.name);
    let state_path = format!("routines/{safe_name}/state.md");
    match ctx.workspace.read(&state_path).await {
        Ok(doc) => Some(doc.content),
        Err(_) => None,
    }
}

/// Assembles the user-facing prompt from the routine prompt, workspace
/// context, previous state, and the ROUTINE_OK sentinel instructions.
fn build_lightweight_prompt(
    prompt: &str,
    context_parts: &[String],
    state_content: Option<&str>,
) -> String {
    let mut full_prompt = String::new();
    full_prompt.push_str(prompt);

    if !context_parts.is_empty() {
        full_prompt.push_str("\n\n---\n\n# Context\n\n");
        full_prompt.push_str(&context_parts.join("\n\n"));
    }

    if let Some(state) = state_content {
        full_prompt.push_str("\n\n---\n\n# Previous State\n\n");
        full_prompt.push_str(state);
    }

    full_prompt.push_str(
        "\n\n---\n\nIf nothing needs attention, reply EXACTLY with: ROUTINE_OK\n\
         If something needs attention, provide a concise summary.",
    );
    full_prompt
}

/// Fetches the workspace system prompt, falling back to empty on failure.
async fn load_system_prompt(ctx: &EngineContext, routine: &Routine) -> String {
    match ctx.workspace.system_prompt().await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(routine = %routine.name, "Failed to get system prompt: {}", e);
            String::new()
        }
    }
}

/// Determines the effective max tokens from model metadata (half the context
/// window), never dropping below the routine's configured value.
async fn resolve_max_tokens(ctx: &EngineContext, max_tokens: u32) -> u32 {
    match ctx.llm.model_metadata().await {
        Ok(meta) => {
            let from_api = meta.context_length.map(|ctx| ctx / 2).unwrap_or(max_tokens);
            from_api.max(max_tokens)
        }
        Err(_) => max_tokens,
    }
}

/// Builds the opening message list: the system prompt (when present)
/// followed by the user prompt.
pub(super) fn initial_messages(system_prompt: &str, full_prompt: &str) -> Vec<ChatMessage> {
    if system_prompt.is_empty() {
        vec![ChatMessage::user(full_prompt)]
    } else {
        vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(full_prompt),
        ]
    }
}

/// Execute a lightweight routine without tool support (original single-call behavior).
async fn execute_lightweight_no_tools(
    ctx: &EngineContext,
    _routine: &Routine,
    system_prompt: &str,
    full_prompt: &str,
    effective_max_tokens: u32,
) -> Result<(RunStatus, Option<String>, Option<i32>), RoutineError> {
    let messages = initial_messages(system_prompt, full_prompt);

    let request = CompletionRequest::new(messages)
        .with_max_tokens(effective_max_tokens)
        .with_temperature(0.3);

    let response = ctx
        .llm
        .complete(request)
        .await
        .map_err(RoutineError::from)?;

    let content = response.content.trim();
    let tokens_used = Some((response.input_tokens + response.output_tokens) as i32);

    // Empty content guard
    if content.is_empty() {
        return if response.finish_reason == FinishReason::Length {
            Err(RoutineError::TruncatedResponse)
        } else {
            Err(RoutineError::EmptyResponse)
        };
    }

    // Check for the "nothing to do" sentinel
    if content == "ROUTINE_OK" || content.contains("ROUTINE_OK") {
        return Ok((RunStatus::Ok, None, tokens_used));
    }

    Ok((RunStatus::Attention, Some(content.to_string()), tokens_used))
}

/// Handle a text-only LLM response in lightweight routine execution.
///
/// Checks for the ROUTINE_OK sentinel, validates content, and returns appropriate status.
pub(super) fn handle_text_response(
    content: &str,
    finish_reason: FinishReason,
    total_input_tokens: u32,
    total_output_tokens: u32,
) -> Result<(RunStatus, Option<String>, Option<i32>), RoutineError> {
    let content = content.trim();

    // Empty content guard
    if content.is_empty() {
        return if finish_reason == FinishReason::Length {
            Err(RoutineError::TruncatedResponse)
        } else {
            Err(RoutineError::EmptyResponse)
        };
    }

    // Check for the "nothing to do" sentinel
    if content == "ROUTINE_OK" || content.contains("ROUTINE_OK") {
        let total_tokens = Some((total_input_tokens + total_output_tokens) as i32);
        return Ok((RunStatus::Ok, None, total_tokens));
    }

    let total_tokens = Some((total_input_tokens + total_output_tokens) as i32);
    Ok((
        RunStatus::Attention,
        Some(content.to_string()),
        total_tokens,
    ))
}
