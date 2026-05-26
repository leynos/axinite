use std::sync::Arc;

use super::super::dispatch::DispatchContext;
use crate::channels::wasm::wrapper::WasmChannel;

#[tokio::test]
async fn test_dispatch_emitted_messages_sends_to_channel() {
    use crate::channels::wasm::host::EmittedMessage;

    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    let message_tx = Arc::new(tokio::sync::RwLock::new(Some(tx)));

    let rate_limiter = Arc::new(tokio::sync::RwLock::new(
        crate::channels::wasm::host::ChannelEmitRateLimiter::new(
            crate::channels::wasm::capabilities::EmitRateLimitConfig::default(),
        ),
    ));

    let messages = vec![
        EmittedMessage::new("user1", "Hello from polling!"),
        EmittedMessage::new("user2", "Another message"),
    ];

    let last_broadcast_metadata = Arc::new(tokio::sync::RwLock::new(None));
    let result = WasmChannel::dispatch_emitted_messages(
        "test-channel",
        messages,
        DispatchContext {
            message_tx: message_tx.as_ref(),
            rate_limiter: rate_limiter.as_ref(),
            last_broadcast_metadata: last_broadcast_metadata.as_ref(),
            settings_store: None,
        },
    )
    .await;

    assert!(result.is_ok());

    // Verify messages were sent
    let msg1 = rx.try_recv().expect("Should receive first message");
    assert_eq!(msg1.user_id, "user1");
    assert_eq!(msg1.content, "Hello from polling!");

    let msg2 = rx.try_recv().expect("Should receive second message");
    assert_eq!(msg2.user_id, "user2");
    assert_eq!(msg2.content, "Another message");

    // No more messages
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn test_dispatch_emitted_messages_no_sender_returns_ok() {
    use crate::channels::wasm::host::EmittedMessage;

    // No sender available (channel not started)
    let message_tx = Arc::new(tokio::sync::RwLock::new(None));
    let rate_limiter = Arc::new(tokio::sync::RwLock::new(
        crate::channels::wasm::host::ChannelEmitRateLimiter::new(
            crate::channels::wasm::capabilities::EmitRateLimitConfig::default(),
        ),
    ));

    let messages = vec![EmittedMessage::new("user1", "Hello!")];

    // Should return Ok even without a sender (logs warning but doesn't fail)
    let last_broadcast_metadata = Arc::new(tokio::sync::RwLock::new(None));
    let result = WasmChannel::dispatch_emitted_messages(
        "test-channel",
        messages,
        DispatchContext {
            message_tx: message_tx.as_ref(),
            rate_limiter: rate_limiter.as_ref(),
            last_broadcast_metadata: last_broadcast_metadata.as_ref(),
            settings_store: None,
        },
    )
    .await;

    assert!(result.is_ok());
}

#[expect(
    clippy::too_many_arguments,
    reason = "test helper mirrors attachment fields to reduce assertion block size"
)]
fn assert_attachment(
    attachment: &crate::channels::IncomingAttachment,
    id: &str,
    mime_type: &str,
    filename: Option<&str>,
    size_bytes: Option<u64>,
    source_url: Option<&str>,
    storage_key: Option<&str>,
    extracted_text: Option<&str>,
) {
    assert_eq!(attachment.id, id);
    assert_eq!(attachment.mime_type, mime_type);
    assert_eq!(attachment.filename.as_deref(), filename);
    assert_eq!(attachment.size_bytes, size_bytes);
    assert_eq!(attachment.source_url.as_deref(), source_url);
    assert_eq!(attachment.storage_key.as_deref(), storage_key);
    assert_eq!(attachment.extracted_text.as_deref(), extracted_text);
}

#[tokio::test]
async fn test_dispatch_emitted_messages_preserves_attachments() {
    use crate::channels::wasm::host::{Attachment, EmittedMessage};

    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    let message_tx = Arc::new(tokio::sync::RwLock::new(Some(tx)));

    let rate_limiter = Arc::new(tokio::sync::RwLock::new(
        crate::channels::wasm::host::ChannelEmitRateLimiter::new(
            crate::channels::wasm::capabilities::EmitRateLimitConfig::default(),
        ),
    ));

    let attachments = vec![
        Attachment {
            id: "photo123".to_string(),
            mime_type: "image/jpeg".to_string(),
            filename: Some("cat.jpg".to_string()),
            size_bytes: Some(50_000),
            source_url: Some("https://api.telegram.org/file/photo123".to_string()),
            storage_key: None,
            extracted_text: None,
            data: Vec::new(),
            duration_secs: None,
        },
        Attachment {
            id: "doc456".to_string(),
            mime_type: "application/pdf".to_string(),
            filename: Some("report.pdf".to_string()),
            size_bytes: Some(120_000),
            source_url: None,
            storage_key: Some("store/doc456".to_string()),
            extracted_text: Some("Report contents...".to_string()),
            data: Vec::new(),
            duration_secs: None,
        },
    ];

    let messages =
        vec![EmittedMessage::new("user1", "Check these files").with_attachments(attachments)];

    let last_broadcast_metadata = Arc::new(tokio::sync::RwLock::new(None));
    let result = WasmChannel::dispatch_emitted_messages(
        "test-channel",
        messages,
        DispatchContext {
            message_tx: message_tx.as_ref(),
            rate_limiter: rate_limiter.as_ref(),
            last_broadcast_metadata: last_broadcast_metadata.as_ref(),
            settings_store: None,
        },
    )
    .await;

    assert!(result.is_ok());

    let msg = rx.try_recv().expect("Should receive message");
    assert_eq!(msg.content, "Check these files");
    assert_eq!(msg.attachments.len(), 2);

    // Verify first attachment
    assert_attachment(
        &msg.attachments[0],
        "photo123",
        "image/jpeg",
        Some("cat.jpg"),
        Some(50_000),
        Some("https://api.telegram.org/file/photo123"),
        None,
        None,
    );

    // Verify second attachment
    assert_attachment(
        &msg.attachments[1],
        "doc456",
        "application/pdf",
        Some("report.pdf"),
        Some(120_000),
        None,
        Some("store/doc456"),
        Some("Report contents..."),
    );
}

#[tokio::test]
async fn test_dispatch_emitted_messages_no_attachments_backward_compat() {
    use crate::channels::wasm::host::EmittedMessage;

    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    let message_tx = Arc::new(tokio::sync::RwLock::new(Some(tx)));

    let rate_limiter = Arc::new(tokio::sync::RwLock::new(
        crate::channels::wasm::host::ChannelEmitRateLimiter::new(
            crate::channels::wasm::capabilities::EmitRateLimitConfig::default(),
        ),
    ));

    let messages = vec![EmittedMessage::new("user1", "Just text, no attachments")];

    let last_broadcast_metadata = Arc::new(tokio::sync::RwLock::new(None));
    let result = WasmChannel::dispatch_emitted_messages(
        "test-channel",
        messages,
        DispatchContext {
            message_tx: message_tx.as_ref(),
            rate_limiter: rate_limiter.as_ref(),
            last_broadcast_metadata: last_broadcast_metadata.as_ref(),
            settings_store: None,
        },
    )
    .await;

    assert!(result.is_ok());

    let msg = rx.try_recv().expect("Should receive message");
    assert_eq!(msg.content, "Just text, no attachments");
    assert!(msg.attachments.is_empty());
}
