/// Print a styled header box.
///
/// # Example
///
/// ```ignore
/// print_header("IronClaw Setup Wizard");
/// ```
pub(super) fn print_header(text: &str) {
    let width = text.len() + 4;
    let border = "─".repeat(width);

    println!();
    println!("╭{}╮", border);
    println!("│  {}  │", text);
    println!("╰{}╯", border);
    println!();
}

/// Print a step indicator.
///
/// # Example
///
/// ```ignore
/// print_step(1, 3, "NEAR AI Authentication");
/// // Output: Step 1/3: NEAR AI Authentication
/// //         ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
/// ```
pub(super) fn print_step(current: usize, total: usize, name: &str) {
    println!("Step {}/{}: {}", current, total, name);
    println!("{}", "━".repeat(32));
    println!();
}
