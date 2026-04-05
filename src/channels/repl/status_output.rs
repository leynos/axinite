//! REPL status-output renderers for user-visible progress, approval cards, and
//! authentication prompts built around `ToolApprovalRequest` and `render_approval_card`.

use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::agent::truncate_for_preview;
use crate::channels::StatusUpdate;

use super::common::sanitize_for_terminal;
use super::formatting::{ToolApprovalRequest, render_approval_card};

/// Max characters for tool result previews in the terminal.
pub(super) const CLI_TOOL_RESULT_MAX: usize = 200;

/// Max characters for thinking/status messages in the terminal.
pub(super) const CLI_STATUS_MAX: usize = 200;

/// Describes a completed tool invocation for terminal rendering.
pub(super) struct ToolCompletedInfo<'a> {
    /// Tool name
    pub name: &'a str,
    /// Whether invocation succeeded
    pub success: bool,
    /// Error message if any
    pub error: Option<&'a str>,
    /// Invocation parameters
    pub parameters: Option<&'a str>,
}

/// Describes a newly started background job for terminal rendering.
pub(super) struct JobStartedInfo<'a> {
    /// Job identifier
    pub job_id: &'a str,
    /// Job title
    pub title: &'a str,
    /// URL to browse the job
    pub browse_url: &'a str,
}

/// Describes an authentication-required event for terminal rendering.
pub(super) struct AuthRequiredInfo<'a> {
    /// Extension name
    pub extension_name: &'a str,
    /// Authentication instructions
    pub instructions: Option<&'a str>,
    /// Setup URL if any
    pub setup_url: Option<&'a str>,
    /// Authentication URL if any
    pub auth_url: Option<&'a str>,
}

/// Describes a completed authentication attempt for terminal rendering.
pub(super) struct AuthCompletedInfo<'a> {
    /// Extension name
    pub extension_name: &'a str,
    /// Whether authentication succeeded
    pub success: bool,
    /// Status message
    pub message: &'a str,
}

fn render_thinking(msg: &str) -> String {
    let display = truncate_for_preview(msg, CLI_STATUS_MAX);
    format!("  \x1b[90m\u{25CB} {display}\x1b[0m")
}

/// Prints a thinking status line to stderr.
///
/// Renders the given message with a hollow circle bullet and gray styling,
/// truncated to CLI_STATUS_MAX if necessary.
pub(super) fn print_thinking(msg: &str) {
    eprintln!("{}", render_thinking(msg));
}

fn render_tool_started(name: &str) -> String {
    format!("  \x1b[33m\u{25CB} {name}\x1b[0m")
}

/// Prints a tool-started status line to stderr.
///
/// Renders the tool name with a yellow hollow circle bullet to indicate
/// the tool has started executing.
pub(super) fn print_tool_started(name: &str) {
    eprintln!("{}", render_tool_started(name));
}

fn render_tool_completed_lines(info: &ToolCompletedInfo<'_>) -> Vec<String> {
    let mut lines = Vec::new();
    let sanitized_name = sanitize_for_terminal(info.name);
    if info.success {
        lines.push(format!("  \x1b[32m\u{25CF} {sanitized_name}\x1b[0m"));
    } else {
        lines.push(format!(
            "  \x1b[31m\u{2717} {sanitized_name} (failed)\x1b[0m"
        ));
        if let Some(error) = info.error {
            let sanitized_error = sanitize_for_terminal(error);
            let display = truncate_for_preview(&sanitized_error, CLI_TOOL_RESULT_MAX);
            lines.push(format!("    \x1b[90merror: {display}\x1b[0m"));
        }
        if let Some(parameters) = info.parameters {
            let sanitized_params = sanitize_for_terminal(parameters);
            let display = truncate_for_preview(&sanitized_params, CLI_TOOL_RESULT_MAX);
            lines.push(format!("    \x1b[90mparams: {display}\x1b[0m"));
        }
    }
    lines
}

/// Prints a tool-completed status (success or failure) to stderr.
///
/// Renders completion info with a green checkmark (success) or red X (failure),
/// optionally including error messages and parameters for failed tools.
pub(super) fn print_tool_completed(info: &ToolCompletedInfo<'_>) {
    for line in render_tool_completed_lines(info) {
        eprintln!("{line}");
    }
}

fn render_tool_result(preview: &str) -> String {
    let display = truncate_for_preview(preview, CLI_TOOL_RESULT_MAX);
    format!("    \x1b[90m{display}\x1b[0m")
}

/// Prints a tool result preview to stderr.
///
/// Renders the result preview with gray styling and indentation,
/// truncated to CLI_TOOL_RESULT_MAX if necessary.
pub(super) fn print_tool_result(preview: &str) {
    eprintln!("{}", render_tool_result(preview));
}

fn render_stream_chunk_separator(width: usize) -> String {
    format!("\x1b[90m{}\x1b[0m", "\u{2500}".repeat(width.min(80)))
}

/// Prints a streaming text chunk to stdout.
///
/// On the first chunk, prints a separator line to stderr. Subsequent chunks
/// are printed directly to stdout and flushed immediately. The `is_streaming`
/// flag tracks whether we've already printed the separator.
pub(super) fn print_stream_chunk(is_streaming: &AtomicBool, chunk: &str) {
    if !is_streaming.swap(true, Ordering::Relaxed) {
        let width = crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(80);
        eprintln!("{}", render_stream_chunk_separator(width));
    }
    print!("{chunk}");
    let _ = io::stdout().flush();
}

fn render_job_started(info: &JobStartedInfo<'_>) -> String {
    let sanitized_title = sanitize_for_terminal(info.title);
    let sanitized_job_id = sanitize_for_terminal(info.job_id);
    let sanitized_url = sanitize_for_terminal(info.browse_url);
    format!(
        "  \x1b[36m[job]\x1b[0m {sanitized_title} \x1b[90m({sanitized_job_id})\x1b[0m \x1b[4m{sanitized_url}\x1b[0m"
    )
}

/// Prints a job-started notification to stderr.
///
/// Renders the job title, ID, and browse URL with appropriate styling
/// to indicate a background job has been spawned.
pub(super) fn print_job_started(info: &JobStartedInfo<'_>) {
    eprintln!("{}", render_job_started(info));
}

fn render_status(is_debug: bool, msg: &str) -> Option<String> {
    let approval_related = msg.to_lowercase().contains("approval");
    if is_debug || approval_related {
        let sanitized_msg = sanitize_for_terminal(msg);
        let display = truncate_for_preview(&sanitized_msg, CLI_STATUS_MAX);
        Some(format!("  \x1b[90m{display}\x1b[0m"))
    } else {
        None
    }
}

/// Prints a general status message to stderr (conditionally).
///
/// Only prints if debug mode is enabled or the message is approval-related.
/// Renders the message with gray styling, truncated to CLI_STATUS_MAX.
pub(super) fn print_status(is_debug: bool, msg: &str) {
    if let Some(line) = render_status(is_debug, msg) {
        eprintln!("{line}");
    }
}

fn render_approval_needed_lines(
    request: &ToolApprovalRequest<'_>,
    parameters: &serde_json::Value,
) -> Vec<String> {
    render_approval_card(request, parameters)
}

/// Prints a tool approval request card to stderr.
///
/// Renders a formatted approval card showing the tool name, description,
/// and parameters, prompting the user to approve or deny execution.
pub(super) fn print_approval_needed(
    request: &ToolApprovalRequest<'_>,
    parameters: &serde_json::Value,
) {
    for line in render_approval_needed_lines(request, parameters) {
        eprintln!("{line}");
    }
}

fn render_auth_required_lines(info: &AuthRequiredInfo<'_>) -> Vec<String> {
    let sanitized_ext_name = sanitize_for_terminal(info.extension_name);
    let mut lines = vec![
        String::new(),
        format!("\x1b[33m  Authentication required for {sanitized_ext_name}\x1b[0m"),
    ];
    if let Some(instr) = info.instructions {
        let sanitized_instr = sanitize_for_terminal(instr);
        lines.push(format!("  {sanitized_instr}"));
    }
    if let Some(url) = info.auth_url {
        let sanitized_url = sanitize_for_terminal(url);
        lines.push(format!("  \x1b[4m{sanitized_url}\x1b[0m"));
    }
    if let Some(url) = info.setup_url
        && Some(url) != info.auth_url
    {
        let sanitized_url = sanitize_for_terminal(url);
        lines.push(format!("  \x1b[4m{sanitized_url}\x1b[0m"));
    }
    lines.push(String::new());
    lines
}

/// Prints an authentication required notification to stderr.
///
/// Renders the extension name, instructions, auth URL, and setup URL
/// to prompt the user to complete authentication.
pub(super) fn print_auth_required(info: &AuthRequiredInfo<'_>) {
    for line in render_auth_required_lines(info) {
        eprintln!("{line}");
    }
}

fn render_auth_completed(info: &AuthCompletedInfo<'_>) -> String {
    let sanitized_ext_name = sanitize_for_terminal(info.extension_name);
    let sanitized_message = sanitize_for_terminal(info.message);
    if info.success {
        format!("\x1b[32m  {sanitized_ext_name}: {sanitized_message}\x1b[0m")
    } else {
        format!("\x1b[31m  {sanitized_ext_name}: {sanitized_message}\x1b[0m")
    }
}

/// Prints an authentication completion message to stderr.
///
/// Renders the extension name and completion message with green (success)
/// or red (failure) styling based on the authentication result.
pub(super) fn print_auth_completed(info: &AuthCompletedInfo<'_>) {
    eprintln!("{}", render_auth_completed(info));
}

fn render_image_generated(path: Option<&str>) -> String {
    if let Some(p) = path {
        let sanitized_path = sanitize_for_terminal(p);
        format!("\x1b[36m  [image] {sanitized_path}\x1b[0m")
    } else {
        "\x1b[36m  [image generated]\x1b[0m".to_string()
    }
}

/// Prints an image generation notification to stderr.
///
/// Renders either the image path or a generic "image generated" message
/// with cyan styling to indicate an image has been created.
pub(super) fn print_image_generated(path: Option<&str>) {
    eprintln!("{}", render_image_generated(path));
}

/// Build a [`ToolApprovalRequest`] from destructured [`StatusUpdate::ApprovalNeeded`]
/// fields and delegate to [`print_approval_needed`].
fn handle_approval_needed(
    request_id: &str,
    tool_name: &str,
    description: &str,
    parameters: &serde_json::Value,
) {
    let request = ToolApprovalRequest {
        request_id,
        tool_name,
        description,
    };
    print_approval_needed(&request, parameters);
}

/// Build an [`AuthRequiredInfo`] from destructured [`StatusUpdate::AuthRequired`]
/// fields and delegate to [`print_auth_required`].
fn handle_auth_required(
    extension_name: &str,
    instructions: Option<&str>,
    setup_url: Option<&str>,
    auth_url: Option<&str>,
) {
    print_auth_required(&AuthRequiredInfo {
        extension_name,
        instructions,
        setup_url,
        auth_url,
    });
}

/// Build a [`ToolCompletedInfo`] from destructured [`StatusUpdate::ToolCompleted`]
/// fields and delegate to [`print_tool_completed`].
fn handle_tool_completed(name: &str, success: bool, error: Option<&str>, parameters: Option<&str>) {
    print_tool_completed(&ToolCompletedInfo {
        name,
        success,
        error,
        parameters,
    });
}

/// Build a [`JobStartedInfo`] from destructured [`StatusUpdate::JobStarted`]
/// fields and delegate to [`print_job_started`].
fn handle_job_started(job_id: &str, title: &str, browse_url: &str) {
    print_job_started(&JobStartedInfo {
        job_id,
        title,
        browse_url,
    });
}

/// Build an [`AuthCompletedInfo`] from destructured [`StatusUpdate::AuthCompleted`]
/// fields and delegate to [`print_auth_completed`].
fn handle_auth_completed(extension_name: &str, success: bool, message: &str) {
    print_auth_completed(&AuthCompletedInfo {
        extension_name,
        success,
        message,
    });
}

/// Route a [`StatusUpdate`] to the appropriate `print_*` helper.
pub(super) fn dispatch_status_update(
    status: StatusUpdate,
    is_streaming: &Arc<AtomicBool>,
    is_debug: bool,
) {
    match status {
        StatusUpdate::Thinking(msg) => print_thinking(&msg),
        StatusUpdate::ToolStarted { name } => print_tool_started(&name),
        StatusUpdate::ToolCompleted {
            name,
            success,
            error,
            parameters,
        } => {
            handle_tool_completed(&name, success, error.as_deref(), parameters.as_deref());
        }
        StatusUpdate::ToolResult { name: _, preview } => print_tool_result(&preview),
        StatusUpdate::StreamChunk(chunk) => print_stream_chunk(is_streaming, &chunk),
        StatusUpdate::JobStarted {
            job_id,
            title,
            browse_url,
        } => {
            handle_job_started(&job_id, &title, &browse_url);
        }
        StatusUpdate::Status(msg) => print_status(is_debug, &msg),
        StatusUpdate::ApprovalNeeded {
            request_id,
            tool_name,
            description,
            parameters,
        } => {
            handle_approval_needed(&request_id, &tool_name, &description, &parameters);
        }
        StatusUpdate::AuthRequired {
            extension_name,
            instructions,
            auth_url,
            setup_url,
        } => {
            handle_auth_required(
                &extension_name,
                instructions.as_deref(),
                setup_url.as_deref(),
                auth_url.as_deref(),
            );
        }
        StatusUpdate::AuthCompleted {
            extension_name,
            success,
            message,
        } => {
            handle_auth_completed(&extension_name, success, &message);
        }
        StatusUpdate::ImageGenerated { path, .. } => print_image_generated(path.as_deref()),
    }
}

#[cfg(test)]
#[path = "status_output_tests.rs"]
mod tests;
