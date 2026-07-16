//! Tests for the `Truncate` compaction strategy and post-compaction
//! message coherence.

use std::sync::Arc;

use uuid::Uuid;

use crate::agent::context_monitor::CompactionStrategy;
use crate::agent::session::Thread;
use crate::testing::StubLlm;

use super::{make_compactor, make_thread};

// ------------------------------------------------------------------
// 1. compact_truncate keeps last N turns
// ------------------------------------------------------------------

#[tokio::test]
async fn test_compact_truncate_keeps_last_n() {
    let llm = Arc::new(StubLlm::new("unused"));
    let compactor = make_compactor(llm);
    let mut thread = make_thread(10);
    assert_eq!(thread.turns.len(), 10);

    let result = compactor
        .compact(
            &mut thread,
            CompactionStrategy::Truncate { keep_recent: 3 },
            None,
        )
        .await
        .expect("compact should succeed");

    // Only 3 turns remain
    assert_eq!(thread.turns.len(), 3);

    // They are the most recent ones (msg-7, msg-8, msg-9)
    assert_eq!(thread.turns[0].user_input, "msg-7");
    assert_eq!(thread.turns[1].user_input, "msg-8");
    assert_eq!(thread.turns[2].user_input, "msg-9");

    // Turn numbers are re-indexed to 0, 1, 2
    assert_eq!(thread.turns[0].turn_number, 0);
    assert_eq!(thread.turns[1].turn_number, 1);
    assert_eq!(thread.turns[2].turn_number, 2);

    // Result metadata
    assert_eq!(result.turns_removed, 7);
    assert!(!result.summary_written);
    assert!(result.summary.is_none());

    // Tokens should be reported (before > 0 since we had content)
    assert!(result.tokens_before > 0);
    assert!(result.tokens_after > 0);
    assert!(result.tokens_before > result.tokens_after);
}

// ------------------------------------------------------------------
// 2. compact_truncate with fewer turns than limit (no-op)
// ------------------------------------------------------------------

#[tokio::test]
async fn test_compact_truncate_with_fewer_turns_than_limit() {
    let llm = Arc::new(StubLlm::new("unused"));
    let compactor = make_compactor(llm);
    let mut thread = make_thread(2);

    let original_inputs: Vec<String> = thread.turns.iter().map(|t| t.user_input.clone()).collect();

    let result = compactor
        .compact(
            &mut thread,
            CompactionStrategy::Truncate { keep_recent: 5 },
            None,
        )
        .await
        .expect("compact should succeed");

    // All turns preserved
    assert_eq!(thread.turns.len(), 2);
    assert_eq!(thread.turns[0].user_input, original_inputs[0]);
    assert_eq!(thread.turns[1].user_input, original_inputs[1]);

    // No turns removed
    assert_eq!(result.turns_removed, 0);
    assert!(!result.summary_written);
    assert!(result.summary.is_none());
}

// ------------------------------------------------------------------
// 3. compact_truncate with empty turns list
// ------------------------------------------------------------------

#[tokio::test]
async fn test_compact_truncate_empty_turns() {
    let llm = Arc::new(StubLlm::new("unused"));
    let compactor = make_compactor(llm);
    let mut thread = Thread::new(Uuid::new_v4());
    assert!(thread.turns.is_empty());

    let result = compactor
        .compact(
            &mut thread,
            CompactionStrategy::Truncate { keep_recent: 3 },
            None,
        )
        .await
        .expect("compact should succeed on empty turns");

    assert!(thread.turns.is_empty());
    assert_eq!(result.turns_removed, 0);
    assert_eq!(result.tokens_before, 0);
    assert_eq!(result.tokens_after, 0);
}

// ------------------------------------------------------------------
// 12. Token counts decrease after truncation
// ------------------------------------------------------------------

#[tokio::test]
async fn test_tokens_decrease_after_compaction() {
    let llm = Arc::new(StubLlm::new("unused"));
    let compactor = make_compactor(llm);
    let mut thread = make_thread(20);

    let result = compactor
        .compact(
            &mut thread,
            CompactionStrategy::Truncate { keep_recent: 5 },
            None,
        )
        .await
        .expect("compact should succeed");

    assert!(
        result.tokens_after < result.tokens_before,
        "tokens_after ({}) should be less than tokens_before ({})",
        result.tokens_after,
        result.tokens_before
    );
}

// ------------------------------------------------------------------
// 13. compact_truncate: keep_recent=0 removes all turns
// ------------------------------------------------------------------

#[tokio::test]
async fn test_compact_truncate_keep_zero() {
    let llm = Arc::new(StubLlm::new("unused"));
    let compactor = make_compactor(llm);
    let mut thread = make_thread(5);

    let result = compactor
        .compact(
            &mut thread,
            CompactionStrategy::Truncate { keep_recent: 0 },
            None,
        )
        .await
        .expect("compact should succeed");

    assert!(thread.turns.is_empty());
    assert_eq!(result.turns_removed, 5);
    assert_eq!(result.tokens_after, 0);
}

// ------------------------------------------------------------------
// 15. Messages are correctly built from turns for thread.messages()
//     after compaction
// ------------------------------------------------------------------

#[tokio::test]
async fn test_messages_coherent_after_compaction() {
    let llm = Arc::new(StubLlm::new("unused"));
    let compactor = make_compactor(llm);
    let mut thread = make_thread(10);

    compactor
        .compact(
            &mut thread,
            CompactionStrategy::Truncate { keep_recent: 3 },
            None,
        )
        .await
        .expect("compact should succeed");

    let messages = thread.messages();
    // 3 turns * 2 messages each (user + assistant) = 6
    assert_eq!(messages.len(), 6);

    // Verify alternating user/assistant pattern
    for (i, msg) in messages.iter().enumerate() {
        if i % 2 == 0 {
            assert_eq!(msg.role, crate::llm::Role::User);
        } else {
            assert_eq!(msg.role, crate::llm::Role::Assistant);
        }
    }

    // Verify content matches the last 3 original turns
    assert_eq!(messages[0].content, "msg-7");
    assert_eq!(messages[1].content, "resp-7");
    assert_eq!(messages[4].content, "msg-9");
    assert_eq!(messages[5].content, "resp-9");
}

// ------------------------------------------------------------------
// 16. Multiple sequential compactions work correctly
// ------------------------------------------------------------------

#[tokio::test]
async fn test_sequential_compactions() {
    let llm = Arc::new(StubLlm::new("unused"));
    let compactor = make_compactor(llm);
    let mut thread = make_thread(20);

    // First compaction: 20 -> 10
    let r1 = compactor
        .compact(
            &mut thread,
            CompactionStrategy::Truncate { keep_recent: 10 },
            None,
        )
        .await
        .expect("first compact");
    assert_eq!(thread.turns.len(), 10);
    assert_eq!(r1.turns_removed, 10);

    // Second compaction: 10 -> 3
    let r2 = compactor
        .compact(
            &mut thread,
            CompactionStrategy::Truncate { keep_recent: 3 },
            None,
        )
        .await
        .expect("second compact");
    assert_eq!(thread.turns.len(), 3);
    assert_eq!(r2.turns_removed, 7);

    // The remaining turns should be the very last 3 from the original 20
    assert_eq!(thread.turns[0].user_input, "msg-17");
    assert_eq!(thread.turns[1].user_input, "msg-18");
    assert_eq!(thread.turns[2].user_input, "msg-19");
}
