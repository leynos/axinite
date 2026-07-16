//! Conversion between provider-neutral chat messages and the Anthropic
//! Messages API wire format.

use crate::llm::provider::{ChatMessage, Role, ToolCall};

use super::{
    AnthropicContent, AnthropicContentBlock, AnthropicMessage, AnthropicResponse,
    AnthropicResponseBlock,
};

/// Convert ChatMessage list to Anthropic format.
///
/// Extracts system messages to the top-level `system` parameter (Anthropic
/// doesn't allow system messages in the `messages` array). Tool-call/tool-result
/// pairs are converted to content blocks.
pub(super) fn convert_messages(
    messages: Vec<ChatMessage>,
) -> (Option<String>, Vec<AnthropicMessage>) {
    let mut system_parts: Vec<String> = Vec::new();
    let mut anthropic_msgs: Vec<AnthropicMessage> = Vec::new();

    for msg in messages {
        match msg.role {
            Role::System => {
                if !msg.content.is_empty() {
                    system_parts.push(msg.content);
                }
            }
            Role::User => {
                anthropic_msgs.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicContent::Text(msg.content),
                });
            }
            Role::Assistant => {
                anthropic_msgs.push(convert_assistant_message(msg));
            }
            Role::Tool => {
                push_tool_result(&mut anthropic_msgs, msg);
            }
        }
    }

    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };

    (system, anthropic_msgs)
}

/// Convert an assistant message: tool calls become content blocks, plain
/// text stays a text message.
fn convert_assistant_message(msg: ChatMessage) -> AnthropicMessage {
    let Some(tool_calls) = msg.tool_calls else {
        return AnthropicMessage {
            role: "assistant".to_string(),
            content: AnthropicContent::Text(msg.content),
        };
    };

    // Assistant message with tool calls → content blocks
    let mut blocks: Vec<AnthropicContentBlock> = Vec::new();
    if !msg.content.is_empty() {
        blocks.push(AnthropicContentBlock::Text { text: msg.content });
    }
    for tc in tool_calls {
        blocks.push(AnthropicContentBlock::ToolUse {
            id: tc.id,
            name: tc.name,
            input: tc.arguments,
        });
    }
    AnthropicMessage {
        role: "assistant".to_string(),
        content: AnthropicContent::Blocks(blocks),
    }
}

/// Convert a tool-result message into a user message with a `tool_result`
/// block, merging consecutive results into one user message as Anthropic
/// requires.
fn push_tool_result(anthropic_msgs: &mut Vec<AnthropicMessage>, msg: ChatMessage) {
    let Some(tool_call_id) = msg.tool_call_id else {
        tracing::warn!("Skipping Tool message without tool_call_id");
        return;
    };
    // Tool results go into a user message with tool_result blocks
    let block = AnthropicContentBlock::ToolResult {
        tool_use_id: tool_call_id,
        content: msg.content,
    };
    // Anthropic requires consecutive tool results in one user message, so
    // append to a trailing user block message when present and start a new
    // user message otherwise.
    if let Some(block) = append_to_trailing_user_blocks(anthropic_msgs, block) {
        anthropic_msgs.push(AnthropicMessage {
            role: "user".to_string(),
            content: AnthropicContent::Blocks(vec![block]),
        });
    }
}

/// Append `block` to the last message when it is a user message carrying
/// content blocks; otherwise hand the block back for a fresh user message.
pub(super) fn append_to_trailing_user_blocks(
    msgs: &mut [AnthropicMessage],
    block: AnthropicContentBlock,
) -> Option<AnthropicContentBlock> {
    let Some(last) = msgs.last_mut() else {
        return Some(block);
    };
    if last.role != "user" {
        return Some(block);
    }
    let AnthropicContent::Blocks(ref mut blocks) = last.content else {
        return Some(block);
    };
    blocks.push(block);
    None
}

/// Extract text content and tool calls from an Anthropic response.
pub(super) fn extract_response_content(
    response: &AnthropicResponse,
) -> (Option<String>, Vec<ToolCall>) {
    let mut text_parts: Vec<String> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();

    for block in &response.content {
        match block {
            AnthropicResponseBlock::Text { text } => {
                text_parts.push(text.clone());
            }
            AnthropicResponseBlock::ToolUse { id, name, input } => {
                tool_calls.push(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: input.clone(),
                });
            }
        }
    }

    let content = if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join(""))
    };

    (content, tool_calls)
}
