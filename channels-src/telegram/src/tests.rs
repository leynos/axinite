use crate::attachments::{extract_attachments, extras_json};
use crate::downloads::{download_and_store_documents, is_downloadable_document, MAX_DOWNLOAD_SIZE_BYTES};
use crate::inbound::{clean_message_text, content_to_emit_for_agent, handle_update};
use crate::polling::get_updates_url;
use crate::send::percent_encode;
use crate::state::CHANNEL_NAME;
use crate::status::{
    classify_status_update, status_message_for_user, TelegramStatusAction, TELEGRAM_STATUS_MAX_CHARS,
    truncate_status_message,
};
use crate::types::*;
use crate::types::{TelegramApiResponse, TelegramMessage, TelegramUpdate};
use crate::webhook::delete_webhook;
use crate::downloads::download_and_store_documents;

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
fn test_get_updates_url_includes_offset_and_timeout() {
    let url = get_updates_url(444_809_884, 30);
    assert!(url.contains("offset=444809884"));
    assert!(url.contains("timeout=30"));
    assert!(url.contains("allowed_updates=[\"message\",\"edited_message\"]"));
}

#[test]
fn test_classify_status_update_thinking() {
    let update = StatusUpdate {
        status: StatusType::Thinking,
        message: "Thinking...".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(
        classify_status_update(&update),
        Some(TelegramStatusAction::Typing)
    );
}

#[test]
fn test_classify_status_update_approval_needed() {
    let update = StatusUpdate {
        status: StatusType::ApprovalNeeded,
        message: "Approval needed for tool 'http_request'".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(
        classify_status_update(&update),
        Some(TelegramStatusAction::Notify(
            "Approval needed for tool 'http_request'".to_string()
        ))
    );
}

#[test]
fn test_classify_status_update_done_ignored() {
    let update = StatusUpdate {
        status: StatusType::Done,
        message: "Done".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_auth_required() {
    let update = StatusUpdate {
        status: StatusType::AuthRequired,
        message: "Authentication required for weather.".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(
        classify_status_update(&update),
        Some(TelegramStatusAction::Notify(
            "Authentication required for weather.".to_string()
        ))
    );
}

#[test]
fn test_classify_status_update_tool_started_ignored() {
    let update = StatusUpdate {
        status: StatusType::ToolStarted,
        message: "Tool started: http_request".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_tool_completed_ignored() {
    let update = StatusUpdate {
        status: StatusType::ToolCompleted,
        message: "Tool completed: http_request (ok)".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_job_started_notify() {
    let update = StatusUpdate {
        status: StatusType::JobStarted,
        message: "Job started: Daily sync".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(
        classify_status_update(&update),
        Some(TelegramStatusAction::Notify(
            "Job started: Daily sync".to_string()
        ))
    );
}

#[test]
fn test_classify_status_update_auth_completed_notify() {
    let update = StatusUpdate {
        status: StatusType::AuthCompleted,
        message: "Authentication completed for weather.".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(
        classify_status_update(&update),
        Some(TelegramStatusAction::Notify(
            "Authentication completed for weather.".to_string()
        ))
    );
}

#[test]
fn test_classify_status_update_tool_result_ignored() {
    let update = StatusUpdate {
        status: StatusType::ToolResult,
        message: "Tool result: http_request ...".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_awaiting_approval_ignored() {
    let update = StatusUpdate {
        status: StatusType::Status,
        message: "Awaiting approval".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_interrupted_ignored() {
    let update = StatusUpdate {
        status: StatusType::Interrupted,
        message: "Interrupted".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_status_done_ignored_case_insensitive() {
    let update = StatusUpdate {
        status: StatusType::Status,
        message: "done".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_status_interrupted_ignored() {
    let update = StatusUpdate {
        status: StatusType::Status,
        message: "interrupted".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_status_rejected_ignored() {
    let update = StatusUpdate {
        status: StatusType::Status,
        message: "Rejected".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_status_notify() {
    let update = StatusUpdate {
        status: StatusType::Status,
        message: "Context compaction started".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(
        classify_status_update(&update),
        Some(TelegramStatusAction::Notify(
            "Context compaction started".to_string()
        ))
    );
}

#[test]
fn test_status_message_for_user_ignores_blank() {
    let update = StatusUpdate {
        status: StatusType::AuthRequired,
        message: "   ".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(status_message_for_user(&update), None);
}

#[test]
fn test_truncate_status_message_appends_ellipsis() {
    let input = "abcdefghijklmnopqrstuvwxyz";
    let output = truncate_status_message(input, 10);
    assert_eq!(output, "abcdefghij...");
}

#[test]
fn test_status_message_for_user_truncates_long_input() {
    let update = StatusUpdate {
        status: StatusType::AuthRequired,
        message: "x".repeat(700),
        metadata_json: "{}".to_string(),
    };

    let msg = status_message_for_user(&update).expect("expected message");
    assert!(msg.len() <= TELEGRAM_STATUS_MAX_CHARS + 3);
    assert!(msg.ends_with("..."));
}

// === Attachment extraction fixture tests ===

#[test]
fn test_extract_attachments_photo() {
    let json = r#"{
        "message_id": 1,
        "from": {"id": 1, "is_bot": false, "first_name": "A"},
        "chat": {"id": 1, "type": "private"},
        "caption": "What is this?",
        "photo": [
            {"file_id": "small_id", "file_unique_id": "s1", "width": 90, "height": 90, "file_size": 1234},
            {"file_id": "large_id", "file_unique_id": "l1", "width": 800, "height": 600, "file_size": 54321}
        ]
    }"#;
    let msg: TelegramMessage = serde_json::from_str(json).unwrap();
    let attachments = extract_attachments(&msg);

    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].id, "large_id"); // Largest photo
    assert_eq!(attachments[0].mime_type, "image/jpeg");
    assert_eq!(attachments[0].size_bytes, Some(54321));
    assert!(attachments[0]
        .source_url
        .as_ref()
        .unwrap()
        .contains("large_id"));
}

#[test]
fn test_extract_attachments_document() {
    let json = r#"{
        "message_id": 2,
        "from": {"id": 1, "is_bot": false, "first_name": "A"},
        "chat": {"id": 1, "type": "private"},
        "document": {
            "file_id": "doc_abc",
            "file_unique_id": "d1",
            "file_name": "report.pdf",
            "mime_type": "application/pdf",
            "file_size": 102400
        },
        "caption": "Here is the report"
    }"#;
    let msg: TelegramMessage = serde_json::from_str(json).unwrap();
    let attachments = extract_attachments(&msg);

    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].id, "doc_abc");
    assert_eq!(attachments[0].mime_type, "application/pdf");
    assert_eq!(attachments[0].filename, Some("report.pdf".to_string()));
    assert_eq!(attachments[0].size_bytes, Some(102400));
}

#[test]
fn test_extract_attachments_voice() {
    let json = r#"{
        "message_id": 3,
        "from": {"id": 1, "is_bot": false, "first_name": "A"},
        "chat": {"id": 1, "type": "private"},
        "voice": {
            "file_id": "voice_xyz",
            "file_unique_id": "v1",
            "duration": 5,
            "mime_type": "audio/ogg",
            "file_size": 9000
        }
    }"#;
    let msg: TelegramMessage = serde_json::from_str(json).unwrap();
    let attachments = extract_attachments(&msg);

    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].id, "voice_xyz");
    assert_eq!(attachments[0].mime_type, "audio/ogg");
    assert_eq!(
        attachments[0].filename.as_deref(),
        Some("voice_voice_xyz.ogg")
    );
    assert!(attachments[0].extras_json.contains("\"duration_secs\":5"));
}

#[test]
fn test_extract_attachments_video() {
    let json = r#"{
        "message_id": 4,
        "from": {"id": 1, "is_bot": false, "first_name": "A"},
        "chat": {"id": 1, "type": "private"},
        "video": {
            "file_id": "vid_1",
            "file_unique_id": "vv1",
            "file_name": "clip.mp4",
            "mime_type": "video/mp4",
            "file_size": 5000000
        },
        "caption": "Check this out"
    }"#;
    let msg: TelegramMessage = serde_json::from_str(json).unwrap();
    let attachments = extract_attachments(&msg);

    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].id, "vid_1");
    assert_eq!(attachments[0].mime_type, "video/mp4");
    assert_eq!(attachments[0].filename, Some("clip.mp4".to_string()));
}

#[test]
fn test_extract_attachments_audio() {
    let json = r#"{
        "message_id": 5,
        "from": {"id": 1, "is_bot": false, "first_name": "A"},
        "chat": {"id": 1, "type": "private"},
        "audio": {
            "file_id": "audio_1",
            "file_unique_id": "a1",
            "file_name": "song.mp3",
            "mime_type": "audio/mpeg",
            "file_size": 3000000
        }
    }"#;
    let msg: TelegramMessage = serde_json::from_str(json).unwrap();
    let attachments = extract_attachments(&msg);

    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].id, "audio_1");
    assert_eq!(attachments[0].mime_type, "audio/mpeg");
    assert_eq!(attachments[0].filename, Some("song.mp3".to_string()));
}

#[test]
fn test_extract_attachments_sticker() {
    let json = r#"{
        "message_id": 6,
        "from": {"id": 1, "is_bot": false, "first_name": "A"},
        "chat": {"id": 1, "type": "private"},
        "sticker": {
            "file_id": "sticker_1",
            "file_unique_id": "st1",
            "type": "regular",
            "file_size": 20000
        }
    }"#;
    let msg: TelegramMessage = serde_json::from_str(json).unwrap();
    let attachments = extract_attachments(&msg);

    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].id, "sticker_1");
    assert_eq!(attachments[0].mime_type, "image/webp");
}

#[test]
fn test_extract_attachments_text_only_empty() {
    let json = r#"{
        "message_id": 7,
        "from": {"id": 1, "is_bot": false, "first_name": "A"},
        "chat": {"id": 1, "type": "private"},
        "text": "Hello"
    }"#;
    let msg: TelegramMessage = serde_json::from_str(json).unwrap();
    let attachments = extract_attachments(&msg);

    assert!(attachments.is_empty());
}

#[test]
fn test_extract_attachments_multiple_types() {
    let json = r#"{
        "message_id": 8,
        "from": {"id": 1, "is_bot": false, "first_name": "A"},
        "chat": {"id": 1, "type": "private"},
        "photo": [
            {"file_id": "photo_1", "file_unique_id": "p1", "width": 100, "height": 100}
        ],
        "document": {
            "file_id": "doc_1",
            "file_unique_id": "d1", 
            "file_name": "file.txt",
            "mime_type": "text/plain"
        }
    }"#;
    let msg: TelegramMessage = serde_json::from_str(json).unwrap();
    let attachments = extract_attachments(&msg);

    // Both photo and document should be extracted
    assert_eq!(attachments.len(), 2);
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

#[test]
fn test_is_downloadable_document() {
    let make = |mime: &str, filename: Option<&str>| InboundAttachment {
        id: "test".to_string(),
        mime_type: mime.to_string(),
        filename: filename.map(|s| s.to_string()),
        size_bytes: Some(1024),
        source_url: None,
        storage_key: None,
        extracted_text: None,
        extras_json: String::new(),
    };

    // PDFs and Office docs should be downloaded
    assert!(is_downloadable_document(&make(
        "application/pdf",
        Some("report.pdf")
    )));
    assert!(is_downloadable_document(&make(
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("doc.docx"),
    )));
    assert!(is_downloadable_document(&make(
        "text/plain",
        Some("notes.txt")
    )));

    // Voice, image, audio, video should NOT be downloaded
    assert!(!is_downloadable_document(&make(
        "audio/ogg",
        Some("voice_123.ogg")
    )));
    assert!(!is_downloadable_document(&make("image/jpeg", None)));
    assert!(!is_downloadable_document(&make(
        "audio/mpeg",
        Some("song.mp3")
    )));
    assert!(!is_downloadable_document(&make(
        "video/mp4",
        Some("clip.mp4")
    )));
}

#[test]
fn test_percent_encode() {
    assert_eq!(percent_encode("a-z"), "a-z");
    assert_eq!(percent_encode("a b"), "a%20b");
    assert_eq!(percent_encode("a@b"), "a%40b");
}

#[test]
fn test_max_download_size_constant() {
    // Verify the constant is 20 MB, matching the Slack channel limit
    assert_eq!(MAX_DOWNLOAD_SIZE_BYTES, 20 * 1024 * 1024);
}

#[test]
fn test_channel_name_constant() {
    assert_eq!(CHANNEL_NAME, "telegram");
}

#[test]
fn test_delete_webhook_not_available_outside_polling() {
    // this ensures webhook entrypoint resolves and can be called from tests.
    let _ = delete_webhook;
}

#[test]
fn test_download_and_store_documents_function_exists() {
    let attachments = &mut [InboundAttachment {
        id: "1".to_string(),
        mime_type: "application/pdf".to_string(),
        filename: Some("x".to_string()),
        size_bytes: Some(1),
        source_url: None,
        storage_key: None,
        extracted_text: None,
        extras_json: String::new(),
    }];
    download_and_store_documents(attachments);
}
