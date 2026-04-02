use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::agent::truncate_for_preview;

use super::formatting::{ToolApprovalRequest, render_approval_card};

/// Max characters for tool result previews in the terminal.
pub(super) const CLI_TOOL_RESULT_MAX: usize = 200;

/// Max characters for thinking/status messages in the terminal.
pub(super) const CLI_STATUS_MAX: usize = 200;

pub(super) fn print_thinking(msg: &str) {
    let display = truncate_for_preview(msg, CLI_STATUS_MAX);
    eprintln!("  \x1b[90m\u{25CB} {display}\x1b[0m");
}

pub(super) fn print_tool_started(name: &str) {
    eprintln!("  \x1b[33m\u{25CB} {name}\x1b[0m");
}

pub(super) fn print_tool_completed(name: &str, success: bool) {
    if success {
        eprintln!("  \x1b[32m\u{25CF} {name}\x1b[0m");
    } else {
        eprintln!("  \x1b[31m\u{2717} {name} (failed)\x1b[0m");
    }
}

pub(super) fn print_tool_result(preview: &str) {
    let display = truncate_for_preview(preview, CLI_TOOL_RESULT_MAX);
    eprintln!("    \x1b[90m{display}\x1b[0m");
}

pub(super) fn print_stream_chunk(is_streaming: &AtomicBool, chunk: &str) {
    if !is_streaming.swap(true, Ordering::Relaxed) {
        let width = crossterm::terminal::size()
            .map(|(w, _)| w as usize)
            .unwrap_or(80);
        eprintln!("\x1b[90m{}\x1b[0m", "\u{2500}".repeat(width.min(80)));
    }
    print!("{chunk}");
    let _ = io::stdout().flush();
}

pub(super) fn print_job_started(job_id: &str, title: &str, browse_url: &str) {
    eprintln!(
        "  \x1b[36m[job]\x1b[0m {title} \x1b[90m({job_id})\x1b[0m \x1b[4m{browse_url}\x1b[0m"
    );
}

pub(super) fn print_status(is_debug: bool, msg: &str) {
    let approval_related = msg.to_lowercase().contains("approval");
    if is_debug || approval_related {
        let display = truncate_for_preview(msg, CLI_STATUS_MAX);
        eprintln!("  \x1b[90m{display}\x1b[0m");
    }
}

pub(super) fn print_approval_needed(
    request: &ToolApprovalRequest<'_>,
    parameters: &serde_json::Value,
) {
    for line in render_approval_card(request, parameters) {
        eprintln!("{line}");
    }
}

pub(super) fn print_auth_required(
    extension_name: &str,
    instructions: Option<&str>,
    setup_url: Option<&str>,
) {
    eprintln!();
    eprintln!("\x1b[33m  Authentication required for {extension_name}\x1b[0m");
    if let Some(instr) = instructions {
        eprintln!("  {instr}");
    }
    if let Some(url) = setup_url {
        eprintln!("  \x1b[4m{url}\x1b[0m");
    }
    eprintln!();
}

pub(super) fn print_auth_completed(extension_name: &str, success: bool, message: &str) {
    if success {
        eprintln!("\x1b[32m  {extension_name}: {message}\x1b[0m");
    } else {
        eprintln!("\x1b[31m  {extension_name}: {message}\x1b[0m");
    }
}

pub(super) fn print_image_generated(path: Option<&str>) {
    if let Some(p) = path {
        eprintln!("\x1b[36m  [image] {p}\x1b[0m");
    } else {
        eprintln!("\x1b[36m  [image generated]\x1b[0m");
    }
}
