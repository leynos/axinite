use std::sync::Arc;

use super::super::dispatch::DispatchContext;
use crate::channels::wasm::wrapper::WasmChannel;

struct RecordingSettingsStore {
    writes: std::sync::Mutex<Vec<String>>,
}

impl RecordingSettingsStore {
    fn new() -> Self {
        Self {
            writes: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn writes(&self) -> Vec<String> {
        self.writes
            .lock()
            .expect("settings writes lock poisoned")
            .clone()
    }
}

impl crate::db::SettingsStore for RecordingSettingsStore {
    fn get_setting<'a>(
        &'a self,
        _user_id: crate::db::UserId,
        _key: crate::db::SettingKey,
    ) -> crate::db::DbFuture<'a, Result<Option<serde_json::Value>, crate::error::DatabaseError>>
    {
        Box::pin(async { Ok(None) })
    }

    fn get_setting_full<'a>(
        &'a self,
        _user_id: crate::db::UserId,
        _key: crate::db::SettingKey,
    ) -> crate::db::DbFuture<
        'a,
        Result<Option<crate::history::SettingRow>, crate::error::DatabaseError>,
    > {
        Box::pin(async { Ok(None) })
    }

    fn set_setting<'a>(
        &'a self,
        _user_id: crate::db::UserId,
        key: crate::db::SettingKey,
        _value: &'a serde_json::Value,
    ) -> crate::db::DbFuture<'a, Result<(), crate::error::DatabaseError>> {
        Box::pin(async move {
            self.writes
                .lock()
                .expect("settings writes lock poisoned")
                .push(key.to_string());
            Ok(())
        })
    }

    fn delete_setting<'a>(
        &'a self,
        _user_id: crate::db::UserId,
        _key: crate::db::SettingKey,
    ) -> crate::db::DbFuture<'a, Result<bool, crate::error::DatabaseError>> {
        Box::pin(async { Ok(false) })
    }

    fn list_settings<'a>(
        &'a self,
        _user_id: crate::db::UserId,
    ) -> crate::db::DbFuture<'a, Result<Vec<crate::history::SettingRow>, crate::error::DatabaseError>>
    {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_all_settings<'a>(
        &'a self,
        _user_id: crate::db::UserId,
    ) -> crate::db::DbFuture<
        'a,
        Result<std::collections::HashMap<String, serde_json::Value>, crate::error::DatabaseError>,
    > {
        Box::pin(async { Ok(std::collections::HashMap::new()) })
    }

    fn set_all_settings<'a>(
        &'a self,
        _user_id: crate::db::UserId,
        _settings: &'a std::collections::HashMap<String, serde_json::Value>,
    ) -> crate::db::DbFuture<'a, Result<(), crate::error::DatabaseError>> {
        Box::pin(async { Ok(()) })
    }

    fn has_settings<'a>(
        &'a self,
        _user_id: crate::db::UserId,
    ) -> crate::db::DbFuture<'a, Result<bool, crate::error::DatabaseError>> {
        Box::pin(async { Ok(false) })
    }
}

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

#[tokio::test]
async fn test_dispatch_emitted_messages_rate_limit_does_not_update_metadata() {
    use crate::channels::wasm::capabilities::EmitRateLimitConfig;
    use crate::channels::wasm::error::WasmChannelError;
    use crate::channels::wasm::host::{ChannelEmitRateLimiter, EmittedMessage};

    let (tx, mut rx) = tokio::sync::mpsc::channel(10);
    let message_tx = Arc::new(tokio::sync::RwLock::new(Some(tx)));
    let rate_limiter = Arc::new(tokio::sync::RwLock::new(ChannelEmitRateLimiter::new(
        EmitRateLimitConfig {
            messages_per_minute: 0,
            messages_per_hour: 0,
        },
    )));
    let last_broadcast_metadata = Arc::new(tokio::sync::RwLock::new(None));
    let metadata_json = r#"{"chat_id":123,"message_id":456}"#;
    let settings_store = Arc::new(RecordingSettingsStore::new());
    let settings_store_dyn: Arc<dyn crate::db::SettingsStore> = settings_store.clone();

    let result = WasmChannel::dispatch_emitted_messages(
        "test-channel",
        vec![EmittedMessage::new("user1", "Hello!").with_metadata(metadata_json)],
        DispatchContext {
            message_tx: message_tx.as_ref(),
            rate_limiter: rate_limiter.as_ref(),
            last_broadcast_metadata: last_broadcast_metadata.as_ref(),
            settings_store: Some(&settings_store_dyn),
        },
    )
    .await;

    assert!(matches!(
        result,
        Err(WasmChannelError::EmitRateLimited { name }) if name == "test-channel"
    ));
    assert!(last_broadcast_metadata.read().await.is_none());
    assert!(settings_store.writes().is_empty());
    assert!(rx.try_recv().is_err());
}

/// Expected field values for one [`crate::channels::IncomingAttachment`], used
/// by [`assert_attachment`] to keep call sites concise and named.
struct ExpectedAttachment<'a> {
    id: &'a str,
    mime_type: &'a str,
    filename: Option<&'a str>,
    size_bytes: Option<u64>,
    source_url: Option<&'a str>,
    storage_key: Option<&'a str>,
    extracted_text: Option<&'a str>,
}

fn assert_attachment(
    attachment: &crate::channels::IncomingAttachment,
    expected: &ExpectedAttachment<'_>,
) {
    assert_eq!(attachment.id, expected.id);
    assert_eq!(attachment.mime_type, expected.mime_type);
    assert_eq!(attachment.filename.as_deref(), expected.filename);
    assert_eq!(attachment.size_bytes, expected.size_bytes);
    assert_eq!(attachment.source_url.as_deref(), expected.source_url);
    assert_eq!(attachment.storage_key.as_deref(), expected.storage_key);
    assert_eq!(
        attachment.extracted_text.as_deref(),
        expected.extracted_text
    );
}

fn build_test_attachments() -> Vec<crate::channels::wasm::host::Attachment> {
    use crate::channels::wasm::host::Attachment;

    vec![
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
    ]
}

async fn dispatch_messages_for_test(
    messages: Vec<crate::channels::wasm::host::EmittedMessage>,
) -> (
    Result<(), crate::channels::wasm::error::WasmChannelError>,
    tokio::sync::mpsc::Receiver<crate::channels::IncomingMessage>,
) {
    let (tx, rx) = tokio::sync::mpsc::channel(10);
    let message_tx = Arc::new(tokio::sync::RwLock::new(Some(tx)));

    let rate_limiter = Arc::new(tokio::sync::RwLock::new(
        crate::channels::wasm::host::ChannelEmitRateLimiter::new(
            crate::channels::wasm::capabilities::EmitRateLimitConfig::default(),
        ),
    ));

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

    (result, rx)
}

fn assert_preserved_attachments(msg: &crate::channels::IncomingMessage) {
    assert_eq!(msg.attachments.len(), 2);

    // Verify first attachment
    assert_attachment(
        &msg.attachments[0],
        &ExpectedAttachment {
            id: "photo123",
            mime_type: "image/jpeg",
            filename: Some("cat.jpg"),
            size_bytes: Some(50_000),
            source_url: Some("https://api.telegram.org/file/photo123"),
            storage_key: None,
            extracted_text: None,
        },
    );

    // Verify second attachment
    assert_attachment(
        &msg.attachments[1],
        &ExpectedAttachment {
            id: "doc456",
            mime_type: "application/pdf",
            filename: Some("report.pdf"),
            size_bytes: Some(120_000),
            source_url: None,
            storage_key: Some("store/doc456"),
            extracted_text: Some("Report contents..."),
        },
    );
}

#[tokio::test]
async fn test_dispatch_emitted_messages_preserves_attachments() {
    use crate::channels::wasm::host::EmittedMessage;

    let messages = vec![
        EmittedMessage::new("user1", "Check these files")
            .with_attachments(build_test_attachments()),
    ];

    let (result, mut rx) = dispatch_messages_for_test(messages).await;

    assert!(result.is_ok());

    let msg = rx.try_recv().expect("Should receive message");
    assert_eq!(msg.content, "Check these files");
    assert_preserved_attachments(&msg);
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
