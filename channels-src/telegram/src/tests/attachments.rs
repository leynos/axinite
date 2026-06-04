use crate::attachments::extract_attachments;
use crate::types::TelegramMessage;

struct ExpectedAttachment<'a> {
    id: &'a str,
    mime_type: &'a str,
    filename: Option<&'a str>,
    size_bytes: Option<Option<u64>>,
    source_url_contains: Option<&'a str>,
    extras_json_contains: Option<&'a str>,
}

fn parse_message(json: &str) -> TelegramMessage {
    serde_json::from_str(json).unwrap()
}

fn single_attachment(json: &str) -> crate::near::agent::channel_host::InboundAttachment {
    let msg = parse_message(json);
    let attachments = extract_attachments(&msg);

    assert_eq!(attachments.len(), 1);

    attachments.into_iter().next().unwrap()
}

fn assert_attachment_matches(
    attachment: &crate::near::agent::channel_host::InboundAttachment,
    expected: ExpectedAttachment<'_>,
) {
    assert_eq!(attachment.id, expected.id);
    assert_eq!(attachment.mime_type, expected.mime_type);
    assert_eq!(attachment.filename.as_deref(), expected.filename);

    if let Some(size_bytes) = expected.size_bytes {
        assert_eq!(attachment.size_bytes, size_bytes);
    }

    if let Some(needle) = expected.source_url_contains {
        assert!(
            attachment
                .source_url
                .as_ref()
                .is_some_and(|url| url.contains(needle)),
            "expected source_url {:?} to contain {:?}",
            attachment.source_url,
            needle
        );
    }

    if let Some(needle) = expected.extras_json_contains {
        assert!(
            attachment.extras_json.contains(needle),
            "expected extras_json {:?} to contain {:?}",
            attachment.extras_json,
            needle
        );
    }
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
    let attachment = single_attachment(json);

    assert_attachment_matches(
        &attachment,
        ExpectedAttachment {
            id: "large_id", // Largest photo
            mime_type: "image/jpeg",
            filename: None,
            size_bytes: Some(Some(54321)),
            source_url_contains: Some("large_id"),
            extras_json_contains: None,
        },
    );
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
    let attachment = single_attachment(json);

    assert_attachment_matches(
        &attachment,
        ExpectedAttachment {
            id: "doc_abc",
            mime_type: "application/pdf",
            filename: Some("report.pdf"),
            size_bytes: Some(Some(102400)),
            source_url_contains: None,
            extras_json_contains: None,
        },
    );
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
    let attachment = single_attachment(json);

    assert_attachment_matches(
        &attachment,
        ExpectedAttachment {
            id: "voice_xyz",
            mime_type: "audio/ogg",
            filename: Some("voice_voice_xyz.ogg"),
            size_bytes: None,
            source_url_contains: None,
            extras_json_contains: Some("\"duration_secs\":5"),
        },
    );
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
    let attachment = single_attachment(json);

    assert_attachment_matches(
        &attachment,
        ExpectedAttachment {
            id: "vid_1",
            mime_type: "video/mp4",
            filename: Some("clip.mp4"),
            size_bytes: None,
            source_url_contains: None,
            extras_json_contains: None,
        },
    );
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
    let attachment = single_attachment(json);

    assert_attachment_matches(
        &attachment,
        ExpectedAttachment {
            id: "audio_1",
            mime_type: "audio/mpeg",
            filename: Some("song.mp3"),
            size_bytes: None,
            source_url_contains: None,
            extras_json_contains: None,
        },
    );
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
