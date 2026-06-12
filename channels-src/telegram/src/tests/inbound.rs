use rstest::rstest;

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
#[rstest]
#[case("/start", None, Some("[User started the bot]"))]
#[case("/Start", None, Some("[User started the bot]"))]
#[case("  /start  ", None, Some("[User started the bot]"))]
#[case("/start hello", None, Some("hello"))]
#[case("/interrupt", None, Some("/interrupt"))]
#[case("/stop", None, Some("/stop"))]
#[case("/help", None, Some("/help"))]
#[case("/undo", None, Some("/undo"))]
#[case("/redo", None, Some("/redo"))]
#[case("/ping", None, Some("/ping"))]
#[case("/tools", None, Some("/tools"))]
#[case("/compact", None, Some("/compact"))]
#[case("/clear", None, Some("/clear"))]
#[case("/version", None, Some("/version"))]
#[case("/approve", None, Some("/approve"))]
#[case("/always", None, Some("/always"))]
#[case("/deny", None, Some("/deny"))]
#[case("/yes", None, Some("/yes"))]
#[case("/no", None, Some("/no"))]
#[case("/help me please", None, Some("me please"))]
#[case("hello world", None, Some("hello world"))]
#[case("just text", None, Some("just text"))]
#[case("", None, None)]
#[case("   ", None, None)]
#[case("@botname", None, None)]
#[case("@alice hello", Some("MyBot"), Some("@alice hello"))]
fn test_content_to_emit_logic(
    #[case] content: &str,
    #[case] bot_username: Option<&str>,
    #[case] expected: Option<&str>,
) {
    assert_eq!(
        content_to_emit_for_agent(content, bot_username),
        expected.map(str::to_string)
    );
}
