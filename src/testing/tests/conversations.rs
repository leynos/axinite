//! Conversation persistence round-trip tests (QA Plan P1 - 2.2).

use crate::db::EnsureConversationParams;
use crate::testing::TestHarnessBuilder;

#[tokio::test]
async fn test_conversation_message_round_trip() {
    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");
    let db = &harness.db;

    let conv_id = db
        .create_conversation("tui", "alice", None)
        .await
        .expect("create conversation");

    // Add several messages in order.
    let m1 = db
        .add_conversation_message(conv_id, "user", "Hello!")
        .await
        .expect("add msg 1");
    let m2 = db
        .add_conversation_message(conv_id, "assistant", "Hi there!")
        .await
        .expect("add msg 2");
    let m3 = db
        .add_conversation_message(conv_id, "user", "How are you?")
        .await
        .expect("add msg 3");

    // IDs must be unique.
    assert_ne!(m1, m2);
    assert_ne!(m2, m3);

    // List messages and verify content + ordering.
    let msgs = db
        .list_conversation_messages(conv_id)
        .await
        .expect("list messages");
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].role, "user");
    assert_eq!(msgs[0].content, "Hello!");
    assert_eq!(msgs[1].role, "assistant");
    assert_eq!(msgs[1].content, "Hi there!");
    assert_eq!(msgs[2].role, "user");
    assert_eq!(msgs[2].content, "How are you?");

    // Timestamps should be monotonically non-decreasing.
    assert!(msgs[0].created_at <= msgs[1].created_at);
    assert!(msgs[1].created_at <= msgs[2].created_at);
}

#[tokio::test]
async fn test_conversation_metadata_persistence() {
    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");
    let db = &harness.db;

    let conv_id = db
        .create_conversation("web", "bob", None)
        .await
        .expect("create conversation");

    // Initially no metadata.
    let meta = db
        .get_conversation_metadata(conv_id)
        .await
        .expect("get metadata");
    // May be None or empty object depending on backend.
    if let Some(m) = &meta {
        assert!(m.is_null() || m.as_object().is_none_or(|o| o.is_empty()));
    }

    // Set a metadata field.
    db.update_conversation_metadata_field(conv_id, "thread_type", &serde_json::json!("assistant"))
        .await
        .expect("set thread_type");

    // Read it back.
    let meta = db
        .get_conversation_metadata(conv_id)
        .await
        .expect("get metadata after update")
        .expect("metadata should exist");
    assert_eq!(meta["thread_type"], "assistant");

    // Update with a second field — first field should still be there.
    db.update_conversation_metadata_field(conv_id, "model", &serde_json::json!("gpt-4"))
        .await
        .expect("set model");

    let meta = db
        .get_conversation_metadata(conv_id)
        .await
        .expect("get metadata after second update")
        .expect("metadata should exist");
    assert_eq!(meta["thread_type"], "assistant");
    assert_eq!(meta["model"], "gpt-4");
}

#[tokio::test]
async fn test_conversation_belongs_to_user() {
    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");
    let db = &harness.db;

    let conv_id = db
        .create_conversation("tui", "alice", None)
        .await
        .expect("create conversation");

    // Owner check should pass.
    assert!(
        db.conversation_belongs_to_user(conv_id, "alice")
            .await
            .expect("belongs check")
    );

    // Different user should NOT own it.
    assert!(
        !db.conversation_belongs_to_user(conv_id, "mallory")
            .await
            .expect("belongs check other user")
    );
}

#[tokio::test]
async fn test_ensure_conversation_idempotent() {
    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");
    let db = &harness.db;

    let conv_id = uuid::Uuid::new_v4();

    // ensure_conversation should create the row.
    db.ensure_conversation(EnsureConversationParams {
        id: conv_id,
        channel: "web",
        user_id: "carol",
        thread_id: None,
    })
    .await
    .expect("ensure first");

    // Calling again with the same ID should not error.
    db.ensure_conversation(EnsureConversationParams {
        id: conv_id,
        channel: "web",
        user_id: "carol",
        thread_id: None,
    })
    .await
    .expect("ensure second (idempotent)");

    // Should be able to add messages to it.
    let msg_id = db
        .add_conversation_message(conv_id, "user", "test message")
        .await
        .expect("add message to ensured conversation");
    assert!(!msg_id.is_nil());

    // Verify the message is there.
    let msgs = db
        .list_conversation_messages(conv_id)
        .await
        .expect("list messages");
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].content, "test message");
}

#[tokio::test]
async fn test_paginated_messages() {
    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");
    let db = &harness.db;

    let conv_id = db
        .create_conversation("tui", "dave", None)
        .await
        .expect("create conversation");

    // Add messages.
    for i in 0..5 {
        db.add_conversation_message(conv_id, "user", &format!("msg {i}"))
            .await
            .expect("add message");
    }

    // First page with limit 3, no cursor. Returns newest-first.
    let (page1, has_more) = db
        .list_conversation_messages_paginated(conv_id, None, 3)
        .await
        .expect("page 1");
    assert_eq!(page1.len(), 3, "first page should have 3 messages");
    assert!(has_more, "should indicate more messages exist");

    // Verify all messages can be retrieved with a large limit.
    let (all, _) = db
        .list_conversation_messages_paginated(conv_id, None, 100)
        .await
        .expect("all messages");
    assert_eq!(all.len(), 5);

    // Messages are returned oldest-first (ascending created_at).
    for w in all.windows(2) {
        assert!(
            w[0].created_at <= w[1].created_at,
            "messages should be in ascending created_at order"
        );
    }
}

#[tokio::test]
async fn test_conversations_with_preview() {
    let harness = TestHarnessBuilder::new()
        .build()
        .await
        .expect("test harness should build");
    let db = &harness.db;

    // Create two conversations for the same user.
    let c1 = db
        .create_conversation("tui", "eve", None)
        .await
        .expect("create c1");
    db.add_conversation_message(c1, "user", "First conversation opener")
        .await
        .expect("add msg to c1");

    let c2 = db
        .create_conversation("tui", "eve", None)
        .await
        .expect("create c2");
    db.add_conversation_message(c2, "user", "Second conversation opener")
        .await
        .expect("add msg to c2");

    // List with preview.
    let summaries = db
        .list_conversations_with_preview("eve", "tui", 10)
        .await
        .expect("list with preview");

    assert_eq!(summaries.len(), 2);
    // Both should have message_count >= 1.
    for s in &summaries {
        assert!(s.message_count >= 1);
    }
}
