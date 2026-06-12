use std::collections::HashMap;

use crate::channels::StatusUpdate;
use crate::channels::wasm::schema::ChannelConfig;

// ============================================================================
// WIT Type Conversion Helpers
// ============================================================================

// Type aliases for the generated WIT types (exported interface)
pub(super) use super::exports::near::agent::channel as wit_channel;

struct StatusText<'a>(&'a str);

struct StatusPreview<'a>(&'a str);

struct ErrorBody<'a>(&'a str);

struct AuthRequiredMessageParts<'a> {
    extension_name: &'a str,
    instructions: &'a Option<String>,
    auth_url: &'a Option<String>,
    setup_url: &'a Option<String>,
}

struct ToolCompletedMessageParts<'a> {
    name: &'a str,
    success: bool,
}

struct ToolResultMessageParts<'a> {
    name: &'a str,
    preview: &'a str,
}

struct ApprovalNeededMessageParts<'a> {
    request_id: &'a str,
    tool_name: &'a str,
    description: &'a str,
}

struct JobStartedMessageParts<'a> {
    job_id: &'a str,
    title: &'a str,
    browse_url: &'a str,
}

struct AuthCompletedMessageParts<'a> {
    extension_name: &'a str,
    success: bool,
    message: &'a str,
}

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
fn truncate_status_text(input: StatusPreview<'_>, max_chars: usize) -> String {
    let mut iter = input.0.chars();
    let truncated: String = iter.by_ref().take(max_chars).collect();
    if iter.next().is_some() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

fn classify_status_string(msg: StatusText<'_>) -> wit_channel::StatusType {
    let trimmed = msg.0.trim();
    if trimmed.eq_ignore_ascii_case("done") {
        wit_channel::StatusType::Done
    } else if trimmed.eq_ignore_ascii_case("interrupted") {
        wit_channel::StatusType::Interrupted
    } else {
        wit_channel::StatusType::Status
    }
}

fn build_auth_required_message(parts: AuthRequiredMessageParts<'_>) -> String {
    let AuthRequiredMessageParts {
        extension_name,
        instructions,
        auth_url,
        setup_url,
    } = parts;

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
}

fn tool_completed_message(parts: ToolCompletedMessageParts<'_>) -> String {
    format!(
        "Tool completed: {} ({})",
        parts.name,
        if parts.success { "ok" } else { "failed" }
    )
}

fn tool_result_message(parts: ToolResultMessageParts<'_>) -> String {
    format!(
        "Tool result: {}\n{}",
        parts.name,
        truncate_status_text(StatusPreview(parts.preview), 280)
    )
}

fn approval_needed_message(parts: ApprovalNeededMessageParts<'_>) -> String {
    format!(
        "Approval needed for tool '{}'. {}\nRequest ID: {}\n\
         Reply with: yes (or /approve), no (or /deny), or always (or /always).",
        parts.tool_name, parts.description, parts.request_id
    )
}

fn job_started_message(parts: JobStartedMessageParts<'_>) -> String {
    format!(
        "Job started: {} ({})\n{}",
        parts.title, parts.job_id, parts.browse_url
    )
}

fn auth_completed_message(parts: AuthCompletedMessageParts<'_>) -> String {
    format!(
        "Authentication {} for {}. {}",
        if parts.success { "completed" } else { "failed" },
        parts.extension_name,
        parts.message
    )
}

fn image_generated_message(path: Option<&String>) -> String {
    match path {
        Some(p) => format!("[image] {}", p),
        None => "[image generated]".to_string(),
    }
}

/// Maps a [`StatusUpdate`] variant to the `(StatusType, message)` pair
/// used by the WIT interface.
fn simple_status_type_and_message(status: &StatusUpdate) -> (wit_channel::StatusType, String) {
    match status {
        StatusUpdate::Thinking(msg) => (wit_channel::StatusType::Thinking, msg.clone()),
        StatusUpdate::StreamChunk(chunk) => (wit_channel::StatusType::Thinking, chunk.clone()),
        StatusUpdate::Status(msg) => (classify_status_string(StatusText(msg)), msg.clone()),
        StatusUpdate::ImageGenerated { path, .. } => (
            wit_channel::StatusType::Status,
            image_generated_message(path.as_ref()),
        ),
        _ => unreachable!("simple_status_type_and_message called with non-simple status"),
    }
}

fn tool_status_type_and_message(status: &StatusUpdate) -> (wit_channel::StatusType, String) {
    match status {
        StatusUpdate::ToolStarted { name } => (
            wit_channel::StatusType::ToolStarted,
            format!("Tool started: {}", name),
        ),
        StatusUpdate::ToolCompleted { name, success, .. } => (
            wit_channel::StatusType::ToolCompleted,
            tool_completed_message(ToolCompletedMessageParts {
                name,
                success: *success,
            }),
        ),
        StatusUpdate::ToolResult { name, preview } => (
            wit_channel::StatusType::ToolResult,
            tool_result_message(ToolResultMessageParts { name, preview }),
        ),
        _ => unreachable!("tool_status_type_and_message called with non-tool status"),
    }
}

fn workflow_status_type_and_message(status: &StatusUpdate) -> (wit_channel::StatusType, String) {
    match status {
        StatusUpdate::ApprovalNeeded {
            request_id,
            tool_name,
            description,
            ..
        } => (
            wit_channel::StatusType::ApprovalNeeded,
            approval_needed_message(ApprovalNeededMessageParts {
                request_id,
                tool_name,
                description,
            }),
        ),
        StatusUpdate::JobStarted {
            job_id,
            title,
            browse_url,
        } => (
            wit_channel::StatusType::JobStarted,
            job_started_message(JobStartedMessageParts {
                job_id,
                title,
                browse_url,
            }),
        ),
        _ => unreachable!("workflow_status_type_and_message called with non-workflow status"),
    }
}

fn auth_status_type_and_message(status: &StatusUpdate) -> (wit_channel::StatusType, String) {
    match status {
        StatusUpdate::AuthRequired {
            extension_name,
            instructions,
            auth_url,
            setup_url,
        } => (
            wit_channel::StatusType::AuthRequired,
            build_auth_required_message(AuthRequiredMessageParts {
                extension_name,
                instructions,
                auth_url,
                setup_url,
            }),
        ),
        StatusUpdate::AuthCompleted {
            extension_name,
            success,
            message,
        } => (
            wit_channel::StatusType::AuthCompleted,
            auth_completed_message(AuthCompletedMessageParts {
                extension_name,
                success: *success,
                message,
            }),
        ),
        _ => unreachable!("auth_status_type_and_message called with non-auth status"),
    }
}

fn status_type_and_message(status: &StatusUpdate) -> (wit_channel::StatusType, String) {
    match status {
        StatusUpdate::Thinking(_)
        | StatusUpdate::StreamChunk(_)
        | StatusUpdate::Status(_)
        | StatusUpdate::ImageGenerated { .. } => simple_status_type_and_message(status),

        StatusUpdate::ToolStarted { .. }
        | StatusUpdate::ToolCompleted { .. }
        | StatusUpdate::ToolResult { .. } => tool_status_type_and_message(status),

        StatusUpdate::ApprovalNeeded { .. } | StatusUpdate::JobStarted { .. } => {
            workflow_status_type_and_message(status)
        }

        StatusUpdate::AuthRequired { .. } | StatusUpdate::AuthCompleted { .. } => {
            auth_status_type_and_message(status)
        }
    }
}

pub(super) fn status_to_wit(
    status: &StatusUpdate,
    metadata: &serde_json::Value,
) -> wit_channel::StatusUpdate {
    let metadata_json = serde_json::to_string(metadata).unwrap_or_default();
    let (status_type, message) = status_type_and_message(status);
    wit_channel::StatusUpdate {
        status: status_type,
        message,
        metadata_json,
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

fn http_error_body(body: ErrorBody<'_>) -> Vec<u8> {
    body.0.as_bytes().to_vec()
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
            body: http_error_body(ErrorBody(message)),
        }
    }
}
