//! Request-side conversion for the Bedrock Converse API: inference
//! configuration, message conversion, and tool configuration.

use aws_sdk_bedrockruntime::types::{
    AnyToolChoice, AutoToolChoice, ContentBlock, ConversationRole, InferenceConfiguration, Message,
    SystemContentBlock, Tool, ToolChoice, ToolConfiguration, ToolInputSchema, ToolResultBlock,
    ToolResultContentBlock, ToolResultStatus, ToolSpecification, ToolUseBlock,
};

use crate::llm::error::LlmError;
use crate::llm::provider::ToolDefinition;

use super::documents::json_to_document;

// ---------------------------------------------------------------------------
// Inference configuration
// ---------------------------------------------------------------------------

/// Build an `InferenceConfiguration` from optional temperature and max_tokens.
/// Returns `None` if neither is set.
pub(super) fn build_inference_config(
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    stop_sequences: Option<&[String]>,
) -> Option<InferenceConfiguration> {
    let mut builder = InferenceConfiguration::builder();
    let mut needs_config = false;

    if let Some(temp) = temperature {
        builder = builder.temperature(temp);
        needs_config = true;
    }
    if let Some(tokens) = max_tokens {
        builder = builder.max_tokens(i32::try_from(tokens).unwrap_or(i32::MAX));
        needs_config = true;
    }
    if let Some(seqs) = stop_sequences
        && !seqs.is_empty()
    {
        builder = builder.set_stop_sequences(Some(seqs.to_vec()));
        needs_config = true;
    }

    if needs_config {
        Some(builder.build())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Message conversion
// ---------------------------------------------------------------------------

/// Convert IronClaw `ChatMessage` list into Bedrock system blocks + messages.
///
/// Key differences from OpenAI/Anthropic protocol:
/// 1. System messages are extracted and passed separately.
/// 2. Tool results (role=Tool) become `ContentBlock::ToolResult` inside User messages.
/// 3. Consecutive tool results are merged into a single User message.
/// 4. Bedrock requires strict user/assistant alternation.
pub(super) fn convert_messages(
    messages: &[crate::llm::provider::ChatMessage],
) -> Result<(Vec<SystemContentBlock>, Vec<Message>), LlmError> {
    use crate::llm::provider::Role;

    let mut system_blocks = Vec::new();
    let mut bedrock_messages: Vec<Message> = Vec::new();
    let mut pending_tool_results: Vec<ContentBlock> = Vec::new();

    for msg in messages {
        match msg.role {
            Role::System => {
                if !msg.content.is_empty() {
                    system_blocks.push(SystemContentBlock::Text(msg.content.clone()));
                }
            }
            Role::User => {
                // Flush any pending tool results as a User message first
                flush_tool_results(&mut pending_tool_results, &mut bedrock_messages)?;

                let content = vec![ContentBlock::Text(msg.content.clone())];
                push_message(&mut bedrock_messages, ConversationRole::User, content)?;
            }
            Role::Assistant => {
                // Flush any pending tool results before an assistant message
                flush_tool_results(&mut pending_tool_results, &mut bedrock_messages)?;

                let content = assistant_content_blocks(msg)?;
                if !content.is_empty() {
                    push_message(&mut bedrock_messages, ConversationRole::Assistant, content)?;
                }
            }
            Role::Tool => {
                // Accumulate tool results — they'll be flushed as a User message
                pending_tool_results.push(tool_result_block(msg)?);
            }
        }
    }

    // Flush any remaining tool results
    flush_tool_results(&mut pending_tool_results, &mut bedrock_messages)?;

    Ok((system_blocks, bedrock_messages))
}

/// Build the content blocks for an assistant message: text (when non-empty)
/// followed by any tool use blocks.
fn assistant_content_blocks(
    msg: &crate::llm::provider::ChatMessage,
) -> Result<Vec<ContentBlock>, LlmError> {
    let mut content = Vec::new();

    if !msg.content.is_empty() {
        content.push(ContentBlock::Text(msg.content.clone()));
    }

    if let Some(ref tool_calls) = msg.tool_calls {
        for tc in tool_calls {
            let input_doc = json_to_document(&tc.arguments);
            let tool_use = ToolUseBlock::builder()
                .tool_use_id(&tc.id)
                .name(&tc.name)
                .input(input_doc)
                .build()
                .map_err(|e| LlmError::RequestFailed {
                    provider: "bedrock".to_string(),
                    reason: format!("Failed to build ToolUseBlock: {}", e),
                })?;
            content.push(ContentBlock::ToolUse(tool_use));
        }
    }

    Ok(content)
}

/// Derive the tool result status from the result payload: a JSON payload
/// with `"is_error": true` reports an error, everything else success.
fn tool_result_status(content: &str) -> Option<ToolResultStatus> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(content) else {
        return Some(ToolResultStatus::Success);
    };
    let is_error = json
        .get("is_error")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if is_error {
        Some(ToolResultStatus::Error)
    } else {
        Some(ToolResultStatus::Success)
    }
}

/// Build a `ToolResult` content block for an accumulated tool message.
fn tool_result_block(msg: &crate::llm::provider::ChatMessage) -> Result<ContentBlock, LlmError> {
    let tool_call_id = msg.tool_call_id.as_deref().unwrap_or("unknown");

    let tool_result = ToolResultBlock::builder()
        .tool_use_id(tool_call_id)
        .content(ToolResultContentBlock::Text(msg.content.clone()))
        .set_status(tool_result_status(&msg.content))
        .build()
        .map_err(|e| LlmError::RequestFailed {
            provider: "bedrock".to_string(),
            reason: format!("Failed to build ToolResultBlock: {}", e),
        })?;

    Ok(ContentBlock::ToolResult(tool_result))
}

/// Flush accumulated tool result blocks as a single User message.
fn flush_tool_results(
    pending: &mut Vec<ContentBlock>,
    messages: &mut Vec<Message>,
) -> Result<(), LlmError> {
    if pending.is_empty() {
        return Ok(());
    }

    let content: Vec<ContentBlock> = std::mem::take(pending);
    push_message(messages, ConversationRole::User, content)?;

    Ok(())
}

/// Push a message, enforcing Bedrock's alternation requirement.
///
/// If the last message has the same role, merge the content blocks into it
/// rather than creating a consecutive same-role message.
fn push_message(
    messages: &mut Vec<Message>,
    role: ConversationRole,
    content: Vec<ContentBlock>,
) -> Result<(), LlmError> {
    if content.is_empty() {
        return Ok(());
    }

    let content = merge_same_role_content(messages, &role, content)?;
    let msg = build_bedrock_message(role, content)?;
    messages.push(msg);

    Ok(())
}

/// Merge `content` with the previous message when it shares `role`.
///
/// Bedrock rejects consecutive same-role messages, so the previous message
/// is removed and its content blocks are prepended to `content`.
fn merge_same_role_content(
    messages: &mut Vec<Message>,
    role: &ConversationRole,
    content: Vec<ContentBlock>,
) -> Result<Vec<ContentBlock>, LlmError> {
    let same_role = messages.last().is_some_and(|last| last.role() == role);
    if !same_role {
        return Ok(content);
    }

    let prev = messages.pop().ok_or_else(|| LlmError::RequestFailed {
        provider: "bedrock".to_string(),
        reason: "Unexpected empty message list during merge".to_string(),
    })?;
    let mut merged = prev.content().to_vec();
    merged.extend(content);
    Ok(merged)
}

/// Build a Bedrock `Message` from a role and content blocks.
fn build_bedrock_message(
    role: ConversationRole,
    content: Vec<ContentBlock>,
) -> Result<Message, LlmError> {
    Message::builder()
        .role(role)
        .set_content(Some(content))
        .build()
        .map_err(|e| LlmError::RequestFailed {
            provider: "bedrock".to_string(),
            reason: format!("Failed to build Message: {}", e),
        })
}

// ---------------------------------------------------------------------------
// Tool configuration
// ---------------------------------------------------------------------------

/// Build Bedrock `ToolConfiguration` from IronClaw tool definitions.
pub(super) fn build_tool_config(
    tools: &[ToolDefinition],
    tool_choice: Option<&str>,
) -> Result<Option<ToolConfiguration>, LlmError> {
    if tools.is_empty() {
        return Ok(None);
    }

    let bedrock_tools: Vec<Tool> = tools
        .iter()
        .map(|td| {
            let input_schema = ToolInputSchema::Json(json_to_document(&td.parameters));
            let spec = ToolSpecification::builder()
                .name(&td.name)
                .description(&td.description)
                .input_schema(input_schema)
                .build()
                .map_err(|e| LlmError::RequestFailed {
                    provider: "bedrock".to_string(),
                    reason: format!("Failed to build ToolSpecification: {}", e),
                })?;
            Ok(Tool::ToolSpec(spec))
        })
        .collect::<Result<Vec<_>, LlmError>>()?;

    let choice = match tool_choice {
        Some("none") => {
            // If tool_choice is "none", don't send tool config at all
            return Ok(None);
        }
        Some("required") => Some(ToolChoice::Any(AnyToolChoice::builder().build())),
        // "auto" or anything else
        _ => Some(ToolChoice::Auto(AutoToolChoice::builder().build())),
    };

    let mut builder = ToolConfiguration::builder().set_tools(Some(bedrock_tools));
    if let Some(c) = choice {
        builder = builder.tool_choice(c);
    }

    let config = builder.build().map_err(|e| LlmError::RequestFailed {
        provider: "bedrock".to_string(),
        reason: format!("Failed to build ToolConfiguration: {}", e),
    })?;

    Ok(Some(config))
}
