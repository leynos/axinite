use super::super::*;

#[test]
fn test_build_turns_from_db_messages_complete() {
    let now = chrono::Utc::now();
    let messages = vec![
        crate::history::ConversationMessage {
            id: uuid::Uuid::new_v4(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            created_at: now,
        },
        crate::history::ConversationMessage {
            id: uuid::Uuid::new_v4(),
            role: "assistant".to_string(),
            content: "Hi there!".to_string(),
            created_at: now + chrono::TimeDelta::seconds(1),
        },
        crate::history::ConversationMessage {
            id: uuid::Uuid::new_v4(),
            role: "user".to_string(),
            content: "How are you?".to_string(),
            created_at: now + chrono::TimeDelta::seconds(2),
        },
        crate::history::ConversationMessage {
            id: uuid::Uuid::new_v4(),
            role: "assistant".to_string(),
            content: "Doing well!".to_string(),
            created_at: now + chrono::TimeDelta::seconds(3),
        },
    ];

    let turns = build_turns_from_db_messages(&messages);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].user_input, "Hello");
    assert_eq!(turns[0].response.as_deref(), Some("Hi there!"));
    assert_eq!(turns[0].state, "Completed");
    assert_eq!(turns[1].user_input, "How are you?");
    assert_eq!(turns[1].response.as_deref(), Some("Doing well!"));
}

#[test]
fn test_build_turns_from_db_messages_incomplete_last() {
    let now = chrono::Utc::now();
    let messages = vec![
        crate::history::ConversationMessage {
            id: uuid::Uuid::new_v4(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            created_at: now,
        },
        crate::history::ConversationMessage {
            id: uuid::Uuid::new_v4(),
            role: "assistant".to_string(),
            content: "Hi!".to_string(),
            created_at: now + chrono::TimeDelta::seconds(1),
        },
        crate::history::ConversationMessage {
            id: uuid::Uuid::new_v4(),
            role: "user".to_string(),
            content: "Lost message".to_string(),
            created_at: now + chrono::TimeDelta::seconds(2),
        },
    ];

    let turns = build_turns_from_db_messages(&messages);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[1].user_input, "Lost message");
    assert!(turns[1].response.is_none());
    assert_eq!(turns[1].state, "Failed");
}

#[test]
fn test_build_turns_from_db_messages_empty() {
    let turns = build_turns_from_db_messages(&[]);
    assert!(turns.is_empty());
}
