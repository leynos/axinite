//! Tests for attachment validation on emitted channel messages.

use crate::channels::wasm::capabilities::ChannelCapabilities;
use crate::channels::wasm::host::{
    Attachment, ChannelHostState, EmittedMessage, MAX_ATTACHMENT_TOTAL_SIZE,
    MAX_ATTACHMENTS_PER_MESSAGE,
};

fn make_attachment(id: &str, mime: &str, size: Option<u64>) -> Attachment {
    Attachment {
        id: id.to_string(),
        mime_type: mime.to_string(),
        filename: None,
        size_bytes: size,
        source_url: None,
        storage_key: None,
        extracted_text: None,
        data: Vec::new(),
        duration_secs: None,
    }
}

#[test]
fn test_emit_message_with_attachments() {
    let caps = ChannelCapabilities::for_channel("test");
    let mut state = ChannelHostState::new("test", caps);

    let msg = EmittedMessage::new("user1", "Check this image")
        .with_attachments(vec![make_attachment("file1", "image/jpeg", Some(1024))]);

    state.emit_message(msg).unwrap();

    let messages = state.take_emitted_messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].attachments.len(), 1);
    assert_eq!(messages[0].attachments[0].id, "file1");
    assert_eq!(messages[0].attachments[0].mime_type, "image/jpeg");
    assert_eq!(messages[0].attachments[0].size_bytes, Some(1024));
}

#[test]
fn test_emit_message_no_attachments_backward_compat() {
    let caps = ChannelCapabilities::for_channel("test");
    let mut state = ChannelHostState::new("test", caps);

    let msg = EmittedMessage::new("user1", "Just text");
    state.emit_message(msg).unwrap();

    let messages = state.take_emitted_messages();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].attachments.is_empty());
}

#[test]
fn test_attachment_count_limit() {
    let caps = ChannelCapabilities::for_channel("test");
    let mut state = ChannelHostState::new("test", caps);

    let attachments: Vec<Attachment> = (0..MAX_ATTACHMENTS_PER_MESSAGE + 5)
        .map(|i| make_attachment(&format!("file{}", i), "image/png", Some(100)))
        .collect();

    let msg = EmittedMessage::new("user1", "Many files").with_attachments(attachments);
    state.emit_message(msg).unwrap();

    let messages = state.take_emitted_messages();
    assert_eq!(messages[0].attachments.len(), MAX_ATTACHMENTS_PER_MESSAGE);
}

#[test]
fn test_attachment_total_size_limit() {
    let caps = ChannelCapabilities::for_channel("test");
    let mut state = ChannelHostState::new("test", caps);

    // Each file is 1/3 of the limit, so 3 fit but 4th does not
    let chunk_size = MAX_ATTACHMENT_TOTAL_SIZE / 3;
    let attachments = vec![
        make_attachment("file1", "image/png", Some(chunk_size)),
        make_attachment("file2", "image/png", Some(chunk_size)),
        make_attachment("file3", "image/png", Some(chunk_size)),
        make_attachment("file4", "image/png", Some(chunk_size)),
    ];

    let msg = EmittedMessage::new("user1", "Big files").with_attachments(attachments);
    state.emit_message(msg).unwrap();

    let messages = state.take_emitted_messages();
    // Only first 3 fit within the total size limit
    assert_eq!(messages[0].attachments.len(), 3);
}

#[test]
fn test_attachment_mime_type_filtering() {
    let caps = ChannelCapabilities::for_channel("test");
    let mut state = ChannelHostState::new("test", caps);

    let attachments = vec![
        make_attachment("ok1", "image/jpeg", Some(100)),
        make_attachment("bad1", "application/x-executable", Some(100)),
        make_attachment("ok2", "application/pdf", Some(100)),
        make_attachment("bad2", "application/x-msdos-program", Some(100)),
        make_attachment("ok3", "text/plain", Some(100)),
        make_attachment("ok4", "audio/mpeg", Some(100)),
        make_attachment("ok5", "video/mp4", Some(100)),
    ];

    let msg = EmittedMessage::new("user1", "Mixed files").with_attachments(attachments);
    state.emit_message(msg).unwrap();

    let messages = state.take_emitted_messages();
    let ids: Vec<&str> = messages[0]
        .attachments
        .iter()
        .map(|a| a.id.as_str())
        .collect();
    assert_eq!(ids, vec!["ok1", "ok2", "ok3", "ok4", "ok5"]);
}

#[test]
fn test_attachment_unknown_size_allowed() {
    let caps = ChannelCapabilities::for_channel("test");
    let mut state = ChannelHostState::new("test", caps);

    let attachments = vec![
        make_attachment("file1", "image/jpeg", None),
        make_attachment("file2", "image/png", None),
    ];

    let msg = EmittedMessage::new("user1", "No sizes").with_attachments(attachments);
    state.emit_message(msg).unwrap();

    let messages = state.take_emitted_messages();
    assert_eq!(messages[0].attachments.len(), 2);
}
