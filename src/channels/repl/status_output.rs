//! REPL status-output renderers for user-visible progress, approval cards, and
//! authentication prompts built around `ToolApprovalRequest` and `render_approval_card`.

use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::agent::truncate_for_preview;
use crate::channels::StatusUpdate;

use super::common::sanitize_for_terminal;
use super::formatting::{ToolApprovalRequest, render_approval_card};

mod auth;

use auth::{AuthCompletedInfo, AuthRequiredInfo, print_auth_completed, print_auth_required};

/// Max characters for tool result previews in the terminal.
pub(super) const CLI_TOOL_RESULT_MAX: usize = 200;

/// Max characters for thinking/status messages in the terminal.
pub(super) const CLI_STATUS_MAX: usize = 200;

/// ANSI sequence for dim gray status text.
const ANSI_GRAY: &str = "\x1b[90m";
/// ANSI sequence that resets terminal styling.
const ANSI_RESET: &str = "\x1b[0m";
/// ANSI sequence for yellow status text.
const ANSI_YELLOW: &str = "\x1b[33m";
/// ANSI sequence for green success text.
const ANSI_GREEN: &str = "\x1b[32m";
/// ANSI sequence for red failure text.
const ANSI_RED: &str = "\x1b[31m";
/// ANSI sequence for cyan status text.
const ANSI_CYAN: &str = "\x1b[36m";
/// ANSI sequence that underlines text.
const ANSI_UNDERLINE: &str = "\x1b[4m";
/// Two-space prefix used to indent top-level status lines.
const STATUS_INDENT: &str = "  ";
/// Four-space prefix used to indent status details.
const DETAIL_INDENT: &str = "    ";

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

fn render_thinking(msg: &str) -> String {
    let display = truncate_for_preview(msg, CLI_STATUS_MAX);
    format!("{STATUS_INDENT}{ANSI_GRAY}\u{25CB} {display}{ANSI_RESET}")
}

/// Prints a thinking status line to stderr.
///
/// Renders the given message with a hollow circle bullet and gray styling,
/// truncated to CLI_STATUS_MAX if necessary.
pub(super) fn print_thinking(msg: &str) {
    eprintln!("{}", render_thinking(msg));
}

fn render_tool_started(name: &str) -> String {
    format!("{STATUS_INDENT}{ANSI_YELLOW}\u{25CB} {name}{ANSI_RESET}")
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
        lines.push(format!(
            "{STATUS_INDENT}{ANSI_GREEN}\u{25CF} {sanitized_name}{ANSI_RESET}"
        ));
    } else {
        lines.push(format!(
            "{STATUS_INDENT}{ANSI_RED}\u{2717} {sanitized_name} (failed){ANSI_RESET}"
        ));
        if let Some(error) = info.error {
            let sanitized_error = sanitize_for_terminal(error);
            let display = truncate_for_preview(&sanitized_error, CLI_TOOL_RESULT_MAX);
            lines.push(format!(
                "{DETAIL_INDENT}{ANSI_GRAY}error: {display}{ANSI_RESET}"
            ));
        }
        if let Some(parameters) = info.parameters {
            let sanitized_params = sanitize_for_terminal(parameters);
            let display = truncate_for_preview(&sanitized_params, CLI_TOOL_RESULT_MAX);
            lines.push(format!(
                "{DETAIL_INDENT}{ANSI_GRAY}params: {display}{ANSI_RESET}"
            ));
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
    format!("{DETAIL_INDENT}{ANSI_GRAY}{display}{ANSI_RESET}")
}

/// Prints a tool result preview to stderr.
///
/// Renders the result preview with gray styling and indentation,
/// truncated to CLI_TOOL_RESULT_MAX if necessary.
pub(super) fn print_tool_result(preview: &str) {
    eprintln!("{}", render_tool_result(preview));
}

fn render_stream_chunk_separator(width: usize) -> String {
    format!(
        "{ANSI_GRAY}{}{ANSI_RESET}",
        "\u{2500}".repeat(width.min(80))
    )
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
        "{STATUS_INDENT}{ANSI_CYAN}[job]{ANSI_RESET} {sanitized_title} {ANSI_GRAY}({sanitized_job_id}){ANSI_RESET} {ANSI_UNDERLINE}{sanitized_url}{ANSI_RESET}"
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
        Some(format!("{STATUS_INDENT}{ANSI_GRAY}{display}{ANSI_RESET}"))
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

fn render_image_generated(path: Option<&str>) -> String {
    if let Some(p) = path {
        let sanitized_path = sanitize_for_terminal(p);
        format!("{ANSI_CYAN}{STATUS_INDENT}[image] {sanitized_path}{ANSI_RESET}")
    } else {
        format!("{ANSI_CYAN}{STATUS_INDENT}[image generated]{ANSI_RESET}")
    }
}

/// Prints an image generation notification to stderr.
///
/// Renders either the image path or a generic "image generated" message
/// with cyan styling to indicate an image has been created.
pub(super) fn print_image_generated(path: Option<&str>) {
    eprintln!("{}", render_image_generated(path));
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
            let info = ToolCompletedInfo {
                name: &name,
                success,
                error: error.as_deref(),
                parameters: parameters.as_deref(),
            };
            print_tool_completed(&info);
        }
        StatusUpdate::ToolResult { name: _, preview } => print_tool_result(&preview),
        StatusUpdate::StreamChunk(chunk) => print_stream_chunk(is_streaming, &chunk),
        StatusUpdate::JobStarted {
            job_id,
            title,
            browse_url,
        } => {
            let info = JobStartedInfo {
                job_id: &job_id,
                title: &title,
                browse_url: &browse_url,
            };
            print_job_started(&info);
        }
        StatusUpdate::Status(msg) => print_status(is_debug, &msg),
        StatusUpdate::ApprovalNeeded {
            request_id,
            tool_name,
            description,
            parameters,
        } => {
            let request = ToolApprovalRequest {
                request_id: &request_id,
                tool_name: &tool_name,
                description: &description,
            };
            print_approval_needed(&request, &parameters);
        }
        StatusUpdate::AuthRequired {
            extension_name,
            instructions,
            auth_url,
            setup_url,
        } => {
            let info = AuthRequiredInfo {
                extension_name: &extension_name,
                instructions: instructions.as_deref(),
                setup_url: setup_url.as_deref(),
                auth_url: auth_url.as_deref(),
            };
            print_auth_required(&info);
        }
        StatusUpdate::AuthCompleted {
            extension_name,
            success,
            message,
        } => {
            let info = AuthCompletedInfo {
                extension_name: &extension_name,
                success,
                message: &message,
            };
            print_auth_completed(&info);
        }
        StatusUpdate::ImageGenerated { path, .. } => print_image_generated(path.as_deref()),
    }
}

#[cfg(test)]
#[path = "status_output_tests.rs"]
mod tests;
