//! Terminal renderers for authentication-required and authentication-completed
//! status updates, including the info structs they consume.

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

pub(super) fn render_auth_completed(info: &AuthCompletedInfo<'_>) -> String {
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

/// Build an [`AuthRequiredInfo`] from destructured [`StatusUpdate::AuthRequired`]
/// fields and delegate to [`print_auth_required`].
///
/// [`StatusUpdate::AuthRequired`]: crate::channels::StatusUpdate::AuthRequired
pub(super) fn handle_auth_required(
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

/// Build an [`AuthCompletedInfo`] from destructured [`StatusUpdate::AuthCompleted`]
/// fields and delegate to [`print_auth_completed`].
///
/// [`StatusUpdate::AuthCompleted`]: crate::channels::StatusUpdate::AuthCompleted
pub(super) fn handle_auth_completed(extension_name: &str, success: bool, message: &str) {
    print_auth_completed(&AuthCompletedInfo {
        extension_name,
        success,
        message,
    });
}
