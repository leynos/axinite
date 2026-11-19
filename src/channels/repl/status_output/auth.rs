//! Terminal renderers for authentication-required and authentication-completed
//! status updates, including the info structs they consume.

use crate::channels::StatusUpdate;
use crate::channels::repl::common::sanitize_for_terminal;

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

pub(super) fn render_auth_required_lines(info: &AuthRequiredInfo<'_>) -> Vec<String> {
    let sanitized_ext_name = sanitize_for_terminal(info.extension_name);
    let mut lines = vec![
        String::new(),
        format!(
            "{ansi_yellow}{status_indent}Authentication required for {sanitized_ext_name}{ansi_reset}",
            ansi_yellow = super::ANSI_YELLOW,
            status_indent = super::STATUS_INDENT,
            ansi_reset = super::ANSI_RESET,
        ),
    ];
    if let Some(instr) = info.instructions {
        let sanitized_instr = sanitize_for_terminal(instr);
        lines.push(format!(
            "{status_indent}{sanitized_instr}",
            status_indent = super::STATUS_INDENT,
        ));
    }
    if let Some(url) = info.auth_url {
        let sanitized_url = sanitize_for_terminal(url);
        lines.push(format!(
            "{status_indent}{ansi_underline}{sanitized_url}{ansi_reset}",
            status_indent = super::STATUS_INDENT,
            ansi_underline = super::ANSI_UNDERLINE,
            ansi_reset = super::ANSI_RESET,
        ));
    }
    if let Some(url) = info.setup_url
        && Some(url) != info.auth_url
    {
        let sanitized_url = sanitize_for_terminal(url);
        lines.push(format!(
            "{status_indent}{ansi_underline}{sanitized_url}{ansi_reset}",
            status_indent = super::STATUS_INDENT,
            ansi_underline = super::ANSI_UNDERLINE,
            ansi_reset = super::ANSI_RESET,
        ));
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

pub(super) fn render_auth_completed(info: &AuthCompletedInfo<'_>) -> String {
    let sanitized_ext_name = sanitize_for_terminal(info.extension_name);
    let sanitized_message = sanitize_for_terminal(info.message);
    if info.success {
        format!(
            "{ansi_green}{status_indent}{sanitized_ext_name}: {sanitized_message}{ansi_reset}",
            ansi_green = super::ANSI_GREEN,
            status_indent = super::STATUS_INDENT,
            ansi_reset = super::ANSI_RESET,
        )
    } else {
        format!(
            "{ansi_red}{status_indent}{sanitized_ext_name}: {sanitized_message}{ansi_reset}",
            ansi_red = super::ANSI_RED,
            status_indent = super::STATUS_INDENT,
            ansi_reset = super::ANSI_RESET,
        )
    }
}

/// Prints an authentication completion message to stderr.
///
/// Renders the extension name and completion message with green (success)
/// or red (failure) styling based on the authentication result.
pub(super) fn print_auth_completed(info: &AuthCompletedInfo<'_>) {
    eprintln!("{}", render_auth_completed(info));
}

/// Renders the authentication variants of [`StatusUpdate`], ignoring any
/// other variant.
///
/// The REPL dispatcher routes only [`StatusUpdate::AuthRequired`] and
/// [`StatusUpdate::AuthCompleted`] here. Keeping the field mapping beside the
/// renderers that consume it leaves the main dispatcher a flat event table.
pub(super) fn dispatch_auth_status(status: &StatusUpdate) {
    match status {
        StatusUpdate::AuthRequired {
            extension_name,
            instructions,
            auth_url,
            setup_url,
        } => {
            let info = AuthRequiredInfo {
                extension_name,
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
                extension_name,
                success: *success,
                message,
            };
            print_auth_completed(&info);
        }
        _ => {}
    }
}
