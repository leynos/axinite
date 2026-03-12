//! Message conversion helpers for the rig adapter.
//!
//! This module turns IronClaw chat history into rig-core message types while
//! preserving tool-call and tool-result semantics expected by downstream
//! providers.

use super::*;

pub(super) fn convert_messages(messages: &[ChatMessage]) -> (Option<String>, Vec<RigMessage>) {
    let mut preamble: Option<String> = None;
    let mut history = Vec::new();

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
            crate::llm::Role::User => {
                if msg.content_parts.is_empty() {
                    history.push(RigMessage::user(&msg.content));
                } else {
                    // Build multimodal user message with text + image parts
                    let mut contents: Vec<UserContent> = vec![UserContent::text(&msg.content)];
                    for part in &msg.content_parts {
                        if let crate::llm::ContentPart::ImageUrl { image_url } = part {
                            // Parse data: URL for base64 images, or use raw URL
                            let image = if let Some(rest) = image_url.url.strip_prefix("data:") {
                                // Format: data:<mime>;base64,<data>
                                let (mime, b64) =
                                    rest.split_once(";base64,").unwrap_or(("image/jpeg", rest));
                                Image {
                                    data: DocumentSourceKind::base64(b64),
                                    media_type: ImageMediaType::from_mime_type(mime),
                                    detail: None,
                                    additional_params: None,
                                }
                            } else {
                                Image {
                                    data: DocumentSourceKind::url(&image_url.url),
                                    media_type: None,
                                    detail: None,
                                    additional_params: None,
                                }
                            };
                            contents.push(UserContent::Image(image));
                        }
                    }
                    if let Ok(many) = OneOrMany::many(contents) {
                        history.push(RigMessage::User { content: many });
                    } else {
                        history.push(RigMessage::user(&msg.content));
                    }
                }
            }
            crate::llm::Role::Assistant => {
                if let Some(ref tool_calls) = msg.tool_calls {
                    // Assistant message with tool calls
                    let mut contents: Vec<AssistantContent> = Vec::new();
                    if !msg.content.is_empty() {
                        contents.push(AssistantContent::text(&msg.content));
                    }
                    for (idx, tc) in tool_calls.iter().enumerate() {
                        let tool_call_id =
                            normalized_tool_call_id(Some(tc.id.as_str()), history.len() + idx);
                        contents.push(AssistantContent::ToolCall(
                            rig::message::ToolCall::new(
                                tool_call_id.clone(),
                                ToolFunction::new(tc.name.clone(), tc.arguments.clone()),
                            )
                            .with_call_id(tool_call_id),
                        ));
                    }
                    if let Ok(many) = OneOrMany::many(contents) {
                        history.push(RigMessage::Assistant {
                            id: None,
                            content: many,
                        });
                    } else {
                        // Shouldn't happen but fall back to text
                        history.push(RigMessage::assistant(&msg.content));
                    }
                } else {
                    history.push(RigMessage::assistant(&msg.content));
                }
            }
            crate::llm::Role::Tool => {
                // Tool result message: wrap as User { ToolResult }
                let tool_id = normalized_tool_call_id(msg.tool_call_id.as_deref(), history.len());
                history.push(RigMessage::User {
                    content: OneOrMany::one(UserContent::ToolResult(RigToolResult {
                        id: tool_id.clone(),
                        call_id: Some(tool_id),
                        content: OneOrMany::one(ToolResultContent::text(&msg.content)),
                    })),
                });
            }
        }
    }

    (preamble, history)
}
