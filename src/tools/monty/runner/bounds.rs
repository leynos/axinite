//! Bounded text helpers shared by the supervisor's protocol paths.

/// Truncates an operator-facing message to a bounded size on a character
/// boundary.
pub(super) fn truncate(message: String) -> String {
    const MAX: usize = 8 * 1024;
    if message.len() <= MAX {
        return message;
    }
    let cut = floor_char_boundary(&message, MAX);
    message[..cut].to_string()
}

/// Appends `text` to `output` without exceeding `limit` bytes.
pub(super) fn append_bounded(output: &mut String, text: &str, limit: usize) {
    let remaining = limit.saturating_sub(output.len());
    if remaining == 0 {
        return;
    }
    let cut = floor_char_boundary(text, remaining.min(text.len()));
    output.push_str(&text[..cut]);
}

/// The largest character boundary at or below `index`.
fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}
