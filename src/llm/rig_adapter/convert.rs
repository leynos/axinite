//! Message conversion helpers for the rig adapter.
//!
//! This module turns Axinite chat history into rig-core message types while
//! preserving tool-call and tool-result semantics expected by downstream
//! providers.

use super::*;

pub(super) fn convert_messages(messages: &[ChatMessage]) -> (Option<String>, Vec<RigMessage>) {
    let mut preamble: Option<String> = None;
    let mut history = Vec::new();
    let mut pending_tool_call_ids = std::collections::VecDeque::new();

    for msg in messages {
        match msg.role {
            crate::llm::Role::System => {
                // Concatenate system messages into preamble
                match preamble {
                    Some(ref mut p) => {
                        p.push('\n');
                        p.push_str(&msg.content);
                    }
                    None => preamble = Some(msg.content.clone()),
                }
            }
            crate::llm::Role::User => history.push(user_message(msg)),
            crate::llm::Role::Assistant => {
                let rig_msg = assistant_message(msg, history.len(), &mut pending_tool_call_ids);
                history.push(rig_msg);
            }
            crate::llm::Role::Tool => {
                let rig_msg = tool_result_message(msg, history.len(), &mut pending_tool_call_ids);
                history.push(rig_msg);
            }
        }
    }

    (preamble, history)
}

/// Build a rig image from a `data:` URL (base64) or a raw URL.
fn image_from_url(url: &str) -> Image {
    if let Some(rest) = url.strip_prefix("data:") {
        // Format: data:<mime>;base64,<data>
        let (mime, b64) = rest.split_once(";base64,").unwrap_or(("image/jpeg", rest));
        Image {
            data: DocumentSourceKind::base64(b64),
            media_type: ImageMediaType::from_mime_type(mime),
            detail: None,
            additional_params: None,
        }
    } else {
        Image {
            data: DocumentSourceKind::url(url),
            media_type: None,
            detail: None,
            additional_params: None,
        }
    }
}

/// Convert a user message; image parts make it a multimodal message.
fn user_message(msg: &ChatMessage) -> RigMessage {
    if msg.content_parts.is_empty() {
        return RigMessage::user(&msg.content);
    }

    // Build multimodal user message with text + image parts
    let mut contents: Vec<UserContent> = vec![UserContent::text(&msg.content)];
    for part in &msg.content_parts {
        if let crate::llm::ContentPart::ImageUrl { image_url } = part {
            contents.push(UserContent::Image(image_from_url(&image_url.url)));
        }
    }
    match OneOrMany::many(contents) {
        Ok(many) => RigMessage::User { content: many },
        Err(_) => RigMessage::user(&msg.content),
    }
}

/// Convert an assistant message, recording the ids of its tool calls as
/// pending so later tool results can pair with them.
///
/// `history_len` is the number of messages already converted.
fn assistant_message(
    msg: &ChatMessage,
    history_len: usize,
    pending_tool_call_ids: &mut std::collections::VecDeque<String>,
) -> RigMessage {
    let Some(ref tool_calls) = msg.tool_calls else {
        return RigMessage::assistant(&msg.content);
    };

    // Assistant message with tool calls
    let mut contents: Vec<AssistantContent> = Vec::new();
    if !msg.content.is_empty() {
        contents.push(AssistantContent::text(&msg.content));
    }
    for (idx, tc) in tool_calls.iter().enumerate() {
        let tool_call_id = normalized_tool_call_id(Some(tc.id.as_str()), history_len + idx);
        pending_tool_call_ids.push_back(tool_call_id.clone());
        contents.push(AssistantContent::ToolCall(
            rig::message::ToolCall::new(
                tool_call_id.clone(),
                ToolFunction::new(tc.name.clone(), tc.arguments.clone()),
            )
            .with_call_id(tool_call_id),
        ));
    }
    match OneOrMany::many(contents) {
        Ok(many) => RigMessage::Assistant {
            id: None,
            content: many,
        },
        // Shouldn't happen but fall back to text
        Err(_) => RigMessage::assistant(&msg.content),
    }
}

/// Convert a tool result: wrap as `User { ToolResult }`, pairing it with its
/// own call id or the oldest pending one.
fn tool_result_message(
    msg: &ChatMessage,
    history_len: usize,
    pending_tool_call_ids: &mut std::collections::VecDeque<String>,
) -> RigMessage {
    let tool_id = msg
        .tool_call_id
        .as_deref()
        .map(|id| normalized_tool_call_id(Some(id), history_len))
        .or_else(|| pending_tool_call_ids.pop_front())
        .unwrap_or_else(|| normalized_tool_call_id(None, history_len));
    RigMessage::User {
        content: OneOrMany::one(UserContent::ToolResult(RigToolResult {
            id: tool_id.clone(),
            call_id: Some(tool_id),
            content: OneOrMany::one(ToolResultContent::text(&msg.content)),
        })),
    }
}
