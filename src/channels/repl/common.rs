//! Common utilities shared across REPL submodules.

/// Sanitize user-controlled strings for safe terminal output.
///
/// Removes ANSI escape sequences, C1 control characters, and normalizes
/// CR/LF to single spaces to prevent terminal spoofing or injection attacks.
pub(super) fn sanitize_for_terminal(text: &str) -> String {
    // First, remove ANSI escape sequences (ESC [ ... m)
    let without_ansi = text
        .split("\x1b[")
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                part.to_string()
            } else {
                // Skip everything until 'm' (end of SGR sequence)
                part.split_once('m')
                    .map(|(_, rest)| rest.to_string())
                    .unwrap_or_default()
            }
        })
        .collect::<String>();

    // Then filter out remaining control characters
    without_ansi
        .chars()
        .filter_map(|c| {
            // Filter out control characters except tab
            if c.is_control() && c != '\t' {
                // Replace CR/LF with space
                if c == '\r' || c == '\n' {
                    Some(' ')
                } else {
                    // Drop other control characters (ESC was already stripped above)
                    None
                }
            } else {
                Some(c)
            }
        })
        .collect()
}
