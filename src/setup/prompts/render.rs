//! Rendering utilities for styled setup-prompt output.
//!
//! Formats bordered headers and numbered step indicators for the interactive
//! setup wizard.

/// Format a styled header box.
///
/// # Example
///
/// ```ignore
/// print_header("Axinite Setup Wizard");
/// ```
pub(super) fn format_header(text: &str) -> String {
    let width = text.len() + 4;
    let border = "─".repeat(width);

    format!("\n╭{border}╮\n│  {text}  │\n╰{border}╯\n\n")
}

/// Format a step indicator.
///
/// # Example
///
/// ```ignore
/// print_step(1, 3, "NEAR AI Authentication");
/// // Output: Step 1/3: NEAR AI Authentication
/// //         ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/// ```
pub(super) fn format_step(current: usize, total: usize, name: &str) -> String {
    format!("Step {current}/{total}: {name}\n{}\n\n", "━".repeat(32))
}
