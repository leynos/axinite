//! Bounded diagnostic and guest-output helpers.

/// Longest diagnostic message the helper sends.
pub(super) const MAX_MESSAGE_BYTES: usize = 8 * 1024;

/// Truncates a diagnostic to the helper's message ceiling.
pub(super) fn truncate(message: &str) -> String {
    if message.len() <= MAX_MESSAGE_BYTES {
        return message.to_owned();
    }
    let mut end = MAX_MESSAGE_BYTES;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &message[..end])
}

/// Splits `text` at or before `limit`, on a character boundary.
///
/// Returns an empty prefix when the first character alone exceeds `limit`.
pub(super) fn split_prefix(text: &str, limit: usize) -> (&str, &str) {
    if text.len() <= limit {
        return (text, "");
    }
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.split_at(end)
}

/// Remaining guest-output bytes for one run.
pub(super) struct OutputBudget {
    remaining: usize,
}

impl OutputBudget {
    pub(super) fn new(limit: usize) -> Self {
        Self { remaining: limit }
    }

    pub(super) fn remaining(&self) -> usize {
        self.remaining
    }

    pub(super) fn consume(&mut self, bytes: usize) {
        self.remaining = self.remaining.saturating_sub(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_respects_utf8_boundaries() {
        let message = "é".repeat(MAX_MESSAGE_BYTES);
        let truncated = truncate(&message);
        assert!(truncated.len() <= MAX_MESSAGE_BYTES + '…'.len_utf8());
        assert!(truncated.ends_with('…'));
    }

    #[test]
    fn short_messages_are_unchanged() {
        assert_eq!(truncate("short"), "short");
    }

    #[test]
    fn split_prefix_never_splits_a_character() {
        let text = "aé";
        let (head, tail) = split_prefix(text, 2);
        assert_eq!(head, "a");
        assert_eq!(tail, "é");
    }

    #[test]
    fn split_prefix_returns_empty_when_a_character_exceeds_the_limit() {
        let (head, tail) = split_prefix("é", 1);
        assert!(head.is_empty());
        assert_eq!(tail, "é");
    }

    #[test]
    fn output_budget_saturates() {
        let mut budget = OutputBudget::new(4);
        budget.consume(10);
        assert_eq!(budget.remaining(), 0);
    }
}
