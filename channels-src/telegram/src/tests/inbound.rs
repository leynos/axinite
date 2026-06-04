use crate::inbound::{clean_message_text, content_to_emit_for_agent};

#[test]
fn test_clean_message_text() {
    // Without bot_username: strips any leading @mention
    assert_eq!(clean_message_text("/start hello", None), "hello");
    assert_eq!(clean_message_text("@bot hello world", None), "hello world");
    assert_eq!(clean_message_text("/start", None), "");
    assert_eq!(clean_message_text("@botname", None), "");
    assert_eq!(clean_message_text("just text", None), "just text");
    assert_eq!(clean_message_text("  spaced  ", None), "spaced");

    // With bot_username: only strips @MyBot, not @alice
    assert_eq!(clean_message_text("@MyBot hello", Some("MyBot")), "hello");
    assert_eq!(clean_message_text("@mybot hi", Some("MyBot")), "hi");
    assert_eq!(
        clean_message_text("@alice hello", Some("MyBot")),
        "@alice hello"
    );
    assert_eq!(clean_message_text("@MyBot", Some("MyBot")), "");
}

#[test]
fn test_clean_message_text_bare_commands() {
    // Bare commands return empty (the caller decides what to emit)
    assert_eq!(clean_message_text("/start", None), "");
    assert_eq!(clean_message_text("/interrupt", None), "");
    assert_eq!(clean_message_text("/stop", None), "");
    assert_eq!(clean_message_text("/help", None), "");
    assert_eq!(clean_message_text("/undo", None), "");
    assert_eq!(clean_message_text("/ping", None), "");

    // Commands with args: command prefix stripped, args returned
    assert_eq!(clean_message_text("/start hello", None), "hello");
    assert_eq!(clean_message_text("/help me please", None), "me please");
    assert_eq!(
        clean_message_text("/model claude-opus-4-6", None),
        "claude-opus-4-6"
    );
}

/// Tests for the content_to_emit logic in handle_message.
/// Since handle_message uses WASM host calls, test the extracted decision function.
#[test]
fn test_content_to_emit_logic() {
    // /start → welcome placeholder
    assert_eq!(
        content_to_emit_for_agent("/start", None),
        Some("[User started the bot]".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("/Start", None),
        Some("[User started the bot]".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("  /start  ", None),
        Some("[User started the bot]".to_string())
    );

    // /start with args → pass args through
    assert_eq!(
        content_to_emit_for_agent("/start hello", None),
        Some("hello".to_string())
    );

    // Control commands → pass through raw so Submission::parse() can match
    assert_eq!(
        content_to_emit_for_agent("/interrupt", None),
        Some("/interrupt".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("/stop", None),
        Some("/stop".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("/help", None),
        Some("/help".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("/undo", None),
        Some("/undo".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("/redo", None),
        Some("/redo".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("/ping", None),
        Some("/ping".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("/tools", None),
        Some("/tools".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("/compact", None),
        Some("/compact".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("/clear", None),
        Some("/clear".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("/version", None),
        Some("/version".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("/approve", None),
        Some("/approve".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("/always", None),
        Some("/always".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("/deny", None),
        Some("/deny".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("/yes", None),
        Some("/yes".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("/no", None),
        Some("/no".to_string())
    );

    // Commands with args → cleaned text (command stripped)
    assert_eq!(
        content_to_emit_for_agent("/help me please", None),
        Some("me please".to_string())
    );

    // Plain text → pass through
    assert_eq!(
        content_to_emit_for_agent("hello world", None),
        Some("hello world".to_string())
    );
    assert_eq!(
        content_to_emit_for_agent("just text", None),
        Some("just text".to_string())
    );

    // Empty / whitespace → skip (None)
    assert_eq!(content_to_emit_for_agent("", None), None);
    assert_eq!(content_to_emit_for_agent("   ", None), None);

    // Bare @mention without bot → skip
    assert_eq!(content_to_emit_for_agent("@botname", None), None);

    // With bot username configured: other mentions are preserved.
    assert_eq!(
        content_to_emit_for_agent("@alice hello", Some("MyBot")),
        Some("@alice hello".to_string())
    );
}
