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
    pub name: &'a str,
    pub success: bool,
    pub error: Option<&'a str>,
    pub parameters: Option<&'a str>,
}

/// Describes a newly started background job for terminal rendering.
pub(super) struct JobStartedInfo<'a> {
    pub job_id: &'a str,
    pub title: &'a str,
    pub browse_url: &'a str,
}

/// Describes an authentication-required event for terminal rendering.
pub(super) struct AuthRequiredInfo<'a> {
    pub extension_name: &'a str,
    pub instructions: Option<&'a str>,
    pub setup_url: Option<&'a str>,
    pub auth_url: Option<&'a str>,
}

/// Describes a completed authentication attempt for terminal rendering.
pub(super) struct AuthCompletedInfo<'a> {
    pub extension_name: &'a str,
    pub success: bool,
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
            print_tool_completed(&ToolCompletedInfo {
                name: &name,
                success,
                error: error.as_deref(),
                parameters: parameters.as_deref(),
            });
        }
        StatusUpdate::ToolResult { name: _, preview } => print_tool_result(&preview),
        StatusUpdate::StreamChunk(chunk) => print_stream_chunk(is_streaming, &chunk),
        StatusUpdate::JobStarted {
            job_id,
            title,
            browse_url,
        } => {
            print_job_started(&JobStartedInfo {
                job_id: &job_id,
                title: &title,
                browse_url: &browse_url,
            });
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
            print_auth_required(&AuthRequiredInfo {
                extension_name: &extension_name,
                instructions: instructions.as_deref(),
                setup_url: setup_url.as_deref(),
                auth_url: auth_url.as_deref(),
            });
        }
        StatusUpdate::AuthCompleted {
            extension_name,
            success,
            message,
        } => {
            print_auth_completed(&AuthCompletedInfo {
                extension_name: &extension_name,
                success,
                message: &message,
            });
        }
        StatusUpdate::ImageGenerated { path, .. } => print_image_generated(path.as_deref()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;

    #[test]
    fn status_output_stdout_snapshot() {
        let stdout = ["stream-one", "stream-two"].join("");
        let separator = render_stream_chunk_separator(80);

        assert_snapshot!(&stdout, @r###"
        stream-onestream-two
        "###);
        assert_snapshot!(&separator, @r###"
        [90m────────────────────────────────────────────────────────────────────────────────[0m
        "###);
    }

    fn tool_output_section() -> String {
        [
            render_thinking("Thinking about the next step"),
            render_tool_started("write_file"),
            render_tool_completed_lines(&ToolCompletedInfo {
                name: "write_file",
                success: true,
                error: None,
                parameters: None,
            })
            .join("\n"),
            render_tool_completed_lines(&ToolCompletedInfo {
                name: "write_file",
                success: false,
                error: Some("permission denied"),
                parameters: Some("{\"path\":\"[REDACTED]\",\"mode\":\"0644\"}"),
            })
            .join("\n"),
            render_tool_result("Wrote file successfully"),
        ]
        .join("\n")
    }

    fn job_status_section() -> String {
        [
            render_job_started(&JobStartedInfo {
                job_id: "job_123",
                title: "Inspect docs",
                browse_url: "https://example.test/job/123",
            }),
            render_status(true, "status for debug").expect("debug status should render"),
            render_status(false, "Approval required for write_file")
                .expect("approval status should render"),
        ]
        .join("\n")
    }

    fn approval_section() -> String {
        let request = ToolApprovalRequest {
            request_id: "req_123456789",
            tool_name: "write_file",
            description: "Write a file after approval",
        };
        render_approval_needed_lines(
            &request,
            &serde_json::json!({"path": "/tmp/example.txt", "mode": "0644"}),
        )
        .join("\n")
    }

    fn auth_image_section() -> String {
        [
            render_auth_required_lines(&AuthRequiredInfo {
                extension_name: "github",
                instructions: Some("Visit the OAuth URL to continue."),
                setup_url: Some("https://example.test/setup"),
                auth_url: Some("https://example.test/auth"),
            })
            .join("\n"),
            render_auth_completed(&AuthCompletedInfo {
                extension_name: "github",
                success: true,
                message: "Authenticated",
            }),
            render_auth_completed(&AuthCompletedInfo {
                extension_name: "github",
                success: false,
                message: "Authentication failed",
            }),
            render_image_generated(Some("/tmp/output.png")),
            render_image_generated(None),
        ]
        .join("\n")
    }

    #[test]
    fn status_output_stderr_snapshot() {
        let stderr = [
            tool_output_section(),
            job_status_section(),
            approval_section(),
            auth_image_section(),
        ]
        .join("\n");

        assert_snapshot!(&stderr, @r###"
          [90m○ Thinking about the next step[0m
          [33m○ write_file[0m
          [32m● write_file[0m
          [31m✗ write_file (failed)[0m
            [90merror: permission denied[0m
            [90mparams: {"path":"[REDACTED]","mode":"0644"}[0m
            [90mWrote file successfully[0m
          [36m[job][0m Inspect docs [90m(job_123)[0m [4mhttps://example.test/job/123[0m
          [90mstatus for debug[0m
          [90mApproval required for write_file[0m

          ┌[33m write_file requires approval [0m─────────────────────────────
          │ [90mWrite a file after approval[0m
          │
          │   [36mmode[0m: [32m"0644"[0m
          │   [36mpath[0m: [32m"/tmp/example.txt"[0m
          │
          │ [32myes[0m (y) / [34malways[0m (a) / [31mno[0m (n)
          └─[90m req_1234 [0m────────────────────────────────────────────────


        [33m  Authentication required for github[0m
          Visit the OAuth URL to continue.
          [4mhttps://example.test/auth[0m
          [4mhttps://example.test/setup[0m

        [32m  github: Authenticated[0m
        [31m  github: Authentication failed[0m
        [36m  [image] /tmp/output.png[0m
        [36m  [image generated][0m
        "###);
    }
}
