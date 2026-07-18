//! Response-side handling for the Bedrock Converse API: content and token
//! extraction, stop-reason mapping, and SDK error mapping.

use aws_sdk_bedrockruntime::operation::converse::ConverseError;
use aws_sdk_bedrockruntime::types::{ContentBlock, StopReason};

use crate::llm::error::LlmError;
use crate::llm::provider::{FinishReason, ToolCall};

use super::documents::document_to_json;

/// Extract text content and tool calls from the Converse response output.
pub(super) fn extract_content_blocks(
    output: Option<&aws_sdk_bedrockruntime::types::ConverseOutput>,
) -> Result<(String, Vec<ToolCall>), LlmError> {
    let output = output.ok_or_else(|| LlmError::RequestFailed {
        provider: "bedrock".to_string(),
        reason: "Converse response has no output".to_string(),
    })?;

    let message = output.as_message().map_err(|_| LlmError::RequestFailed {
        provider: "bedrock".to_string(),
        reason: "Converse output is not a message".to_string(),
    })?;

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in message.content() {
        match block {
            ContentBlock::Text(t) => {
                text_parts.push(t.clone());
            }
            ContentBlock::ToolUse(tu) => {
                tool_calls.push(ToolCall {
                    id: tu.tool_use_id().to_string(),
                    name: tu.name().to_string(),
                    arguments: document_to_json(tu.input()),
                });
            }
            // Ignore reasoning, citations, images, etc.
            _ => {}
        }
    }

    Ok((text_parts.join(""), tool_calls))
}

/// Extract token usage from the response, converting i32 → u32 safely.
pub(super) fn extract_token_usage(
    usage: Option<&aws_sdk_bedrockruntime::types::TokenUsage>,
) -> (u32, u32) {
    match usage {
        Some(u) => (
            u32::try_from(u.input_tokens()).unwrap_or(0),
            u32::try_from(u.output_tokens()).unwrap_or(0),
        ),
        None => (0, 0),
    }
}

/// Map Bedrock `StopReason` to Axinite `FinishReason`.
pub(super) fn map_stop_reason(reason: &StopReason) -> FinishReason {
    match reason {
        StopReason::EndTurn | StopReason::StopSequence => FinishReason::Stop,
        StopReason::ToolUse => FinishReason::ToolUse,
        StopReason::MaxTokens | StopReason::ModelContextWindowExceeded => FinishReason::Length,
        StopReason::ContentFiltered | StopReason::GuardrailIntervened => {
            FinishReason::ContentFilter
        }
        _ => FinishReason::Unknown,
    }
}

/// Map AWS SDK errors to `LlmError`.
pub(super) fn map_sdk_error<R: std::fmt::Debug>(
    error: &aws_sdk_bedrockruntime::error::SdkError<ConverseError, R>,
) -> LlmError {
    use aws_sdk_bedrockruntime::error::SdkError;

    match error {
        SdkError::ServiceError(service_err) => {
            let msg = match service_err.err() {
                ConverseError::ModelTimeoutException(e) => {
                    format!("Model timeout: {}", e.message().unwrap_or("unknown"))
                }
                ConverseError::ModelNotReadyException(e) => {
                    format!("Model not ready: {}", e.message().unwrap_or("unknown"))
                }
                ConverseError::ThrottlingException(e) => {
                    format!("Throttled: {}", e.message().unwrap_or("unknown"))
                }
                ConverseError::ValidationException(e) => {
                    format!("Validation error: {}", e.message().unwrap_or("unknown"))
                }
                ConverseError::AccessDeniedException(e) => {
                    format!("Access denied: {}", e.message().unwrap_or("unknown"))
                }
                ConverseError::ResourceNotFoundException(e) => {
                    format!("Resource not found: {}", e.message().unwrap_or("unknown"))
                }
                ConverseError::ModelErrorException(e) => {
                    format!("Model error: {}", e.message().unwrap_or("unknown"))
                }
                ConverseError::InternalServerException(e) => {
                    format!(
                        "Internal server error: {}",
                        e.message().unwrap_or("unknown")
                    )
                }
                ConverseError::ServiceUnavailableException(e) => {
                    format!("Service unavailable: {}", e.message().unwrap_or("unknown"))
                }
                _ => format!("Bedrock service error: {}", service_err.err()),
            };
            LlmError::RequestFailed {
                provider: "bedrock".to_string(),
                reason: msg,
            }
        }
        SdkError::TimeoutError(_) => LlmError::RequestFailed {
            provider: "bedrock".to_string(),
            reason: "Request timed out".to_string(),
        },
        SdkError::DispatchFailure(e) => LlmError::RequestFailed {
            provider: "bedrock".to_string(),
            reason: format!("Connection error: {:?}", e),
        },
        _ => LlmError::RequestFailed {
            provider: "bedrock".to_string(),
            reason: format!("AWS SDK error: {}", error),
        },
    }
}
