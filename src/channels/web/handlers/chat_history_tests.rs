//! Tests for chat-history turn reconstruction helpers.

use rstest::rstest;
use uuid::Uuid;

use crate::channels::web::util::build_turns_from_db_messages;

fn make_message(
    role: &str,
    content: &str,
    offset: chrono::TimeDelta,
    base: chrono::DateTime<chrono::Utc>,
) -> crate::history::ConversationMessage {
    crate::history::ConversationMessage {
        id: Uuid::new_v4(),
        role: role.to_string(),
        content: content.to_string(),
        created_at: base + offset,
    }
}

#[rstest]
#[case::complete(
    vec![
        ("user", "Hello", 0),
        ("assistant", "Hi there!", 1),
        ("user", "How are you?", 2),
        ("assistant", "Doing well!", 3),
    ],
    vec![
        ("Hello", Some("Hi there!"), "Completed"),
        ("How are you?", Some("Doing well!"), "Completed"),
    ]
)]
#[case::incomplete_last(
    vec![
        ("user", "Hello", 0),
        ("assistant", "Hi!", 1),
        ("user", "Lost message", 2),
    ],
    vec![
        ("Hello", Some("Hi!"), "Completed"),
        ("Lost message", None, "Failed"),
    ]
)]
fn test_build_turns_from_db_messages(
    #[case] rows: Vec<(&str, &str, i64)>,
    #[case] expected: Vec<(&str, Option<&str>, &str)>,
) {
    let now = chrono::Utc::now();
    let messages: Vec<_> = rows
        .into_iter()
        .map(|(role, content, offset)| {
            make_message(role, content, chrono::TimeDelta::seconds(offset), now)
        })
        .collect();

    let turns = build_turns_from_db_messages(&messages);
    assert_eq!(turns.len(), expected.len());

    for (turn, (user_input, response, state)) in turns.iter().zip(expected.iter()) {
        assert_eq!(turn.user_input, *user_input);
        assert_eq!(turn.response.as_deref(), *response);
        assert_eq!(turn.state, *state);
    }
}

#[test]
fn test_build_turns_with_tool_calls() {
    let now = chrono::Utc::now();
    let tool_calls_json = serde_json::json!([
        {"name": "shell", "result_preview": "file1.txt\nfile2.txt"},
        {"name": "http", "error": "timeout"}
    ]);
    let messages = vec![
        make_message("user", "List files", chrono::TimeDelta::zero(), now),
        make_message(
            "tool_calls",
            &tool_calls_json.to_string(),
            chrono::TimeDelta::milliseconds(500),
            now,
        ),
        make_message(
            "assistant",
            "Here are the files",
            chrono::TimeDelta::seconds(1),
            now,
        ),
    ];

    let turns = build_turns_from_db_messages(&messages);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].tool_calls.len(), 2);
    assert_eq!(turns[0].tool_calls[0].name, "shell");
    assert!(turns[0].tool_calls[0].has_result);
    assert!(!turns[0].tool_calls[0].has_error);
    assert_eq!(
        turns[0].tool_calls[0].result_preview.as_deref(),
        Some("file1.txt\nfile2.txt")
    );
    assert_eq!(turns[0].tool_calls[1].name, "http");
    assert!(turns[0].tool_calls[1].has_error);
    assert_eq!(turns[0].tool_calls[1].error.as_deref(), Some("timeout"));
    assert_eq!(turns[0].response.as_deref(), Some("Here are the files"));
    assert_eq!(turns[0].state, "Completed");
}

#[test]
fn test_build_turns_with_malformed_tool_calls() {
    let now = chrono::Utc::now();
    let messages = vec![
        make_message("user", "Hello", chrono::TimeDelta::zero(), now),
        make_message(
            "tool_calls",
            "not valid json",
            chrono::TimeDelta::milliseconds(500),
            now,
        ),
        make_message("assistant", "Done", chrono::TimeDelta::seconds(1), now),
    ];

    let turns = build_turns_from_db_messages(&messages);
    assert_eq!(turns.len(), 1);
    assert!(turns[0].tool_calls.is_empty());
    assert_eq!(turns[0].response.as_deref(), Some("Done"));
}

#[test]
fn test_build_turns_backward_compatible_no_tool_calls() {
    let now = chrono::Utc::now();
    let messages = vec![
        make_message("user", "Hello", chrono::TimeDelta::zero(), now),
        make_message("assistant", "Hi!", chrono::TimeDelta::seconds(1), now),
    ];

    let turns = build_turns_from_db_messages(&messages);
    assert_eq!(turns.len(), 1);
    assert!(turns[0].tool_calls.is_empty());
    assert_eq!(turns[0].response.as_deref(), Some("Hi!"));
    assert_eq!(turns[0].state, "Completed");
}
