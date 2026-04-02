//! REPL status-output renderers for user-visible progress, approval cards, and
//! authentication prompts built around `ToolApprovalRequest` and `render_approval_card`.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::agent::truncate_for_preview;

use super::formatting::{ToolApprovalRequest, render_approval_card};

/// Max characters for tool result previews in the terminal.
pub(super) const CLI_TOOL_RESULT_MAX: usize = 200;

/// Max characters for thinking/status messages in the terminal.
pub(super) const CLI_STATUS_MAX: usize = 200;

fn render_thinking(msg: &str) -> String {
    let display = truncate_for_preview(msg, CLI_STATUS_MAX);
    format!("  \x1b[90m\u{25CB} {display}\x1b[0m")
}

pub(super) fn print_thinking(msg: &str) {
    eprintln!("{}", render_thinking(msg));
}

fn render_tool_started(name: &str) -> String {
    format!("  \x1b[33m\u{25CB} {name}\x1b[0m")
}

pub(super) fn print_tool_started(name: &str) {
    eprintln!("{}", render_tool_started(name));
}

fn render_tool_completed_lines(
    name: &str,
    success: bool,
    error: Option<&str>,
    parameters: Option<&str>,
) -> Vec<String> {
    let mut lines = Vec::new();
    if success {
        lines.push(format!("  \x1b[32m\u{25CF} {name}\x1b[0m"));
    } else {
        lines.push(format!("  \x1b[31m\u{2717} {name} (failed)\x1b[0m"));
        if let Some(error) = error {
            let display = truncate_for_preview(error, CLI_TOOL_RESULT_MAX);
            lines.push(format!("    \x1b[90merror: {display}\x1b[0m"));
        }
        if let Some(parameters) = parameters {
            let display = truncate_for_preview(parameters, CLI_TOOL_RESULT_MAX);
            lines.push(format!("    \x1b[90mparams: {display}\x1b[0m"));
        }
    }
    lines
}

pub(super) fn print_tool_completed(
    name: &str,
    success: bool,
    error: Option<&str>,
    parameters: Option<&str>,
) {
    for line in render_tool_completed_lines(name, success, error, parameters) {
        eprintln!("{line}");
    }
}

fn render_tool_result(preview: &str) -> String {
    let display = truncate_for_preview(preview, CLI_TOOL_RESULT_MAX);
    format!("    \x1b[90m{display}\x1b[0m")
}

pub(super) fn print_tool_result(preview: &str) {
    eprintln!("{}", render_tool_result(preview));
}

fn render_stream_chunk_separator(width: usize) -> String {
    format!("\x1b[90m{}\x1b[0m", "\u{2500}".repeat(width.min(80)))
}

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

fn render_job_started(job_id: &str, title: &str, browse_url: &str) -> String {
    format!("  \x1b[36m[job]\x1b[0m {title} \x1b[90m({job_id})\x1b[0m \x1b[4m{browse_url}\x1b[0m")
}

pub(super) fn print_job_started(job_id: &str, title: &str, browse_url: &str) {
    eprintln!("{}", render_job_started(job_id, title, browse_url));
}

fn render_status(is_debug: bool, msg: &str) -> Option<String> {
    let approval_related = msg.to_lowercase().contains("approval");
    if is_debug || approval_related {
        let display = truncate_for_preview(msg, CLI_STATUS_MAX);
        Some(format!("  \x1b[90m{display}\x1b[0m"))
    } else {
        None
    }
}

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

pub(super) fn print_approval_needed(
    request: &ToolApprovalRequest<'_>,
    parameters: &serde_json::Value,
) {
    for line in render_approval_needed_lines(request, parameters) {
        eprintln!("{line}");
    }
}

fn render_auth_required_lines(
    extension_name: &str,
    instructions: Option<&str>,
    setup_url: Option<&str>,
    auth_url: Option<&str>,
) -> Vec<String> {
    let mut lines = vec![
        String::new(),
        format!("\x1b[33m  Authentication required for {extension_name}\x1b[0m"),
    ];
    if let Some(instr) = instructions {
        lines.push(format!("  {instr}"));
    }
    if let Some(url) = auth_url {
        lines.push(format!("  \x1b[4m{url}\x1b[0m"));
    }
    if let Some(url) = setup_url
        && Some(url) != auth_url
    {
        lines.push(format!("  \x1b[4m{url}\x1b[0m"));
    }
    lines.push(String::new());
    lines
}

pub(super) fn print_auth_required(
    extension_name: &str,
    instructions: Option<&str>,
    setup_url: Option<&str>,
    auth_url: Option<&str>,
) {
    for line in render_auth_required_lines(extension_name, instructions, setup_url, auth_url) {
        eprintln!("{line}");
    }
}

fn render_auth_completed(extension_name: &str, success: bool, message: &str) -> String {
    if success {
        format!("\x1b[32m  {extension_name}: {message}\x1b[0m")
    } else {
        format!("\x1b[31m  {extension_name}: {message}\x1b[0m")
    }
}

pub(super) fn print_auth_completed(extension_name: &str, success: bool, message: &str) {
    eprintln!(
        "{}",
        render_auth_completed(extension_name, success, message)
    );
}

fn render_image_generated(path: Option<&str>) -> String {
    if let Some(p) = path {
        format!("\x1b[36m  [image] {p}\x1b[0m")
    } else {
        "\x1b[36m  [image generated]\x1b[0m".to_string()
    }
}

pub(super) fn print_image_generated(path: Option<&str>) {
    eprintln!("{}", render_image_generated(path));
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

    #[test]
    fn status_output_stderr_snapshot() {
        let approval_request = ToolApprovalRequest {
            request_id: "req_123456789",
            tool_name: "write_file",
            description: "Write a file after approval",
        };
        let stderr = [
            render_thinking("Thinking about the next step"),
            render_tool_started("write_file"),
            render_tool_completed_lines("write_file", true, None, None).join("\n"),
            render_tool_completed_lines(
                "write_file",
                false,
                Some("permission denied"),
                Some("{\"path\":\"[REDACTED]\",\"mode\":\"0644\"}"),
            )
            .join("\n"),
            render_tool_result("Wrote file successfully"),
            render_job_started("job_123", "Inspect docs", "https://example.test/job/123"),
            render_status(true, "status for debug").expect("debug status should render"),
            render_status(false, "Approval required for write_file")
                .expect("approval status should render"),
            render_approval_needed_lines(
                &approval_request,
                &serde_json::json!({"path": "/tmp/example.txt", "mode": "0644"}),
            )
            .join("\n"),
            render_auth_required_lines(
                "github",
                Some("Visit the OAuth URL to continue."),
                Some("https://example.test/setup"),
                Some("https://example.test/auth"),
            )
            .join("\n"),
            render_auth_completed("github", true, "Authenticated"),
            render_auth_completed("github", false, "Authentication failed"),
            render_image_generated(Some("/tmp/output.png")),
            render_image_generated(None),
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
