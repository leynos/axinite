use crate::attachments::extract_attachments;
use crate::types::{TelegramConfig, TelegramMessage, TelegramUpdate};

#[test]
fn test_config_with_owner_id() {
    let json = r#"{"owner_id": 123456789}"#;
    let config: TelegramConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.owner_id, Some(123456789));
}

#[test]
fn test_config_without_owner_id() {
    let json = r#"{}"#;
    let config: TelegramConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.owner_id, None);
}

#[test]
fn test_config_with_null_owner_id() {
    let json = r#"{"owner_id": null}"#;
    let config: TelegramConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.owner_id, None);
}

#[test]
fn test_config_full() {
    let json = r#"{
        "bot_username": "my_bot",
        "owner_id": 42,
        "respond_to_all_group_messages": true
    }"#;
    let config: TelegramConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.bot_username, Some("my_bot".to_string()));
    assert_eq!(config.owner_id, Some(42));
    assert!(config.respond_to_all_group_messages);
}

#[test]
fn test_parse_update() {
    let json = r#"{
        "update_id": 123,
        "message": {
            "message_id": 456,
            "from": {
                "id": 789,
                "is_bot": false,
                "first_name": "John",
                "last_name": "Doe"
            },
            "chat": {
                "id": 789,
                "type": "private"
            },
            "text": "Hello bot"
        }
    }"#;

    let update: TelegramUpdate = serde_json::from_str(json).unwrap();
    assert_eq!(update.update_id, 123);

    let message = update.message.unwrap();
    assert_eq!(message.message_id, 456);
    assert_eq!(message.text.unwrap(), "Hello bot");

    let from = message.from.unwrap();
    assert_eq!(from.id, 789);
    assert_eq!(from.first_name, "John");
}

#[test]
fn test_parse_message_with_caption() {
    let json = r#"{
        "message_id": 1,
        "from": {"id": 1, "is_bot": false, "first_name": "A"},
        "chat": {"id": 1, "type": "private"},
        "caption": "What's in this image?"
    }"#;
    let msg: TelegramMessage = serde_json::from_str(json).unwrap();
    assert_eq!(msg.text, None);
    assert_eq!(msg.caption.as_deref(), Some("What's in this image?"));
}

#[test]
fn test_parse_update_with_photo_fallback_content() {
    // A photo-only message (no text, no caption) should have empty content
    // but still produce attachments
    let json = r#"{
        "message_id": 9,
        "from": {"id": 42, "is_bot": false, "first_name": "Test"},
        "chat": {"id": 42, "type": "private"},
        "photo": [
            {"file_id": "ph1", "file_unique_id": "u1", "width": 320, "height": 240}
        ]
    }"#;
    let msg: TelegramMessage = serde_json::from_str(json).unwrap();

    // Content is empty (no text, no caption)
    assert!(msg.text.is_none());
    assert!(msg.caption.is_none());

    // But attachments exist
    let attachments = extract_attachments(&msg);
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].id, "ph1");
}
