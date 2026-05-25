use std::collections::HashMap;

use crate::channels::StatusUpdate;
use crate::channels::wasm::schema::ChannelConfig;

// ============================================================================
// WIT Type Conversion Helpers
// ============================================================================

// Type aliases for the generated WIT types (exported interface)
pub(super) use super::exports::near::agent::channel as wit_channel;

/// Convert WIT-generated ChannelConfig to our internal type.
pub(super) fn convert_channel_config(wit: wit_channel::ChannelConfig) -> ChannelConfig {
    ChannelConfig {
        display_name: wit.display_name,
        http_endpoints: wit
            .http_endpoints
            .into_iter()
            .map(
                |ep| crate::channels::wasm::schema::HttpEndpointConfigSchema {
                    path: ep.path,
                    methods: ep.methods,
                    require_secret: ep.require_secret,
                },
            )
            .collect(),
        poll: wit
            .poll
            .map(|p| crate::channels::wasm::schema::PollConfigSchema {
                interval_ms: p.interval_ms,
                enabled: p.enabled,
            }),
    }
}

/// Convert WIT-generated OutgoingHttpResponse to our HttpResponse type.
pub(super) fn convert_http_response(wit: wit_channel::OutgoingHttpResponse) -> HttpResponse {
    let headers = serde_json::from_str(&wit.headers_json).unwrap_or_default();
    HttpResponse {
        status: wit.status,
        headers,
        body: wit.body,
    }
}

/// Convert a StatusUpdate + metadata into the WIT StatusUpdate type.
pub(super) fn truncate_status_text(input: &str, max_chars: usize) -> String {
    let mut iter = input.chars();
    let truncated: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

pub(super) fn status_to_wit(
    status: &StatusUpdate,
    metadata: &serde_json::Value,
) -> wit_channel::StatusUpdate {
    let metadata_json = serde_json::to_string(metadata).unwrap_or_default();

    match status {
        StatusUpdate::Thinking(msg) => wit_channel::StatusUpdate {
            status: wit_channel::StatusType::Thinking,
            message: msg.clone(),
            metadata_json,
        },
        StatusUpdate::ToolStarted { name } => wit_channel::StatusUpdate {
            status: wit_channel::StatusType::ToolStarted,
            message: format!("Tool started: {}", name),
            metadata_json,
        },
        StatusUpdate::ToolCompleted { name, success, .. } => wit_channel::StatusUpdate {
            status: wit_channel::StatusType::ToolCompleted,
            message: format!(
                "Tool completed: {} ({})",
                name,
                if *success { "ok" } else { "failed" }
            ),
            metadata_json,
        },
        StatusUpdate::ToolResult { name, preview } => wit_channel::StatusUpdate {
            status: wit_channel::StatusType::ToolResult,
            message: format!(
                "Tool result: {}\n{}",
                name,
                truncate_status_text(preview, 280)
            ),
            metadata_json,
        },
        StatusUpdate::StreamChunk(chunk) => wit_channel::StatusUpdate {
            status: wit_channel::StatusType::Thinking,
            message: chunk.clone(),
            metadata_json,
        },
        StatusUpdate::Status(msg) => {
            // Map well-known status strings to WIT types (case-insensitive
            // to stay consistent with is_terminal_text_status and the
            // Telegram-side classify_status_update).
            let trimmed = msg.trim();
            let status_type = if trimmed.eq_ignore_ascii_case("done") {
                wit_channel::StatusType::Done
            } else if trimmed.eq_ignore_ascii_case("interrupted") {
                wit_channel::StatusType::Interrupted
            } else {
                wit_channel::StatusType::Status
            };
            wit_channel::StatusUpdate {
                status: status_type,
                message: msg.clone(),
                metadata_json,
            }
        }
        StatusUpdate::ApprovalNeeded {
            request_id,
            tool_name,
            description,
            ..
        } => wit_channel::StatusUpdate {
            status: wit_channel::StatusType::ApprovalNeeded,
            message: format!(
                "Approval needed for tool '{}'. {}\nRequest ID: {}\nReply with: yes (or /approve), no (or /deny), or always (or /always).",
                tool_name, description, request_id
            ),
            metadata_json,
        },
        StatusUpdate::JobStarted {
            job_id,
            title,
            browse_url,
        } => wit_channel::StatusUpdate {
            status: wit_channel::StatusType::JobStarted,
            message: format!("Job started: {} ({})\n{}", title, job_id, browse_url),
            metadata_json,
        },
        StatusUpdate::AuthRequired {
            extension_name,
            instructions,
            auth_url,
            setup_url,
        } => wit_channel::StatusUpdate {
            status: wit_channel::StatusType::AuthRequired,
            message: {
                let mut lines = vec![format!("Authentication required for {}.", extension_name)];
                if let Some(text) = instructions
                    && !text.trim().is_empty()
                {
                    lines.push(text.trim().to_string());
                }
                if let Some(url) = auth_url {
                    lines.push(format!("Auth URL: {}", url));
                }
                if let Some(url) = setup_url {
                    lines.push(format!("Setup URL: {}", url));
                }
                lines.join("\n")
            },
            metadata_json,
        },
        StatusUpdate::AuthCompleted {
            extension_name,
            success,
            message,
        } => wit_channel::StatusUpdate {
            status: wit_channel::StatusType::AuthCompleted,
            message: format!(
                "Authentication {} for {}. {}",
                if *success { "completed" } else { "failed" },
                extension_name,
                message
            ),
            metadata_json,
        },
        StatusUpdate::ImageGenerated { path, .. } => wit_channel::StatusUpdate {
            status: wit_channel::StatusType::Status,
            message: match path {
                Some(p) => format!("[image] {}", p),
                None => "[image generated]".to_string(),
            },
            metadata_json,
        },
    }
}

/// Clone a WIT StatusUpdate (the generated type doesn't derive Clone).
pub(super) fn clone_wit_status_update(
    update: &wit_channel::StatusUpdate,
) -> wit_channel::StatusUpdate {
    wit_channel::StatusUpdate {
        status: match update.status {
            wit_channel::StatusType::Thinking => wit_channel::StatusType::Thinking,
            wit_channel::StatusType::Done => wit_channel::StatusType::Done,
            wit_channel::StatusType::Interrupted => wit_channel::StatusType::Interrupted,
            wit_channel::StatusType::ToolStarted => wit_channel::StatusType::ToolStarted,
            wit_channel::StatusType::ToolCompleted => wit_channel::StatusType::ToolCompleted,
            wit_channel::StatusType::ToolResult => wit_channel::StatusType::ToolResult,
            wit_channel::StatusType::ApprovalNeeded => wit_channel::StatusType::ApprovalNeeded,
            wit_channel::StatusType::Status => wit_channel::StatusType::Status,
            wit_channel::StatusType::JobStarted => wit_channel::StatusType::JobStarted,
            wit_channel::StatusType::AuthRequired => wit_channel::StatusType::AuthRequired,
            wit_channel::StatusType::AuthCompleted => wit_channel::StatusType::AuthCompleted,
        },
        message: update.message.clone(),
        metadata_json: update.metadata_json.clone(),
    }
}

/// HTTP response from a WASM channel callback.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response headers.
    pub headers: HashMap<String, String>,
    /// Response body.
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Create an OK response.
    pub fn ok() -> Self {
        Self {
            status: 200,
            headers: HashMap::new(),
            body: Vec::new(),
        }
    }

    /// Create a JSON response.
    pub fn json(value: serde_json::Value) -> Self {
        let body = serde_json::to_vec(&value).unwrap_or_default();
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        Self {
            status: 200,
            headers,
            body,
        }
    }

    /// Create an error response.
    pub fn error(status: u16, message: &str) -> Self {
        Self {
            status,
            headers: HashMap::new(),
            body: message.as_bytes().to_vec(),
        }
    }
}
