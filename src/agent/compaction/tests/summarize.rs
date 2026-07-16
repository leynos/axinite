//! Tests for the `Summarize` compaction strategy.

use std::sync::Arc;

use crate::agent::context_monitor::CompactionStrategy;
use crate::testing::StubLlm;

use super::{make_compactor, make_thread};

// ------------------------------------------------------------------
// 4. compact_with_summary produces summary turn via StubLlm
// ------------------------------------------------------------------

#[tokio::test]
async fn test_compact_with_summary_produces_summary_turn() {
    let canned_summary =
        "- User greeted the agent\n- Agent responded warmly\n- Five exchanges completed";
    let llm = Arc::new(StubLlm::new(canned_summary));
    let compactor = make_compactor(llm.clone());
    let mut thread = make_thread(5);

    let result = compactor
        .compact(
            &mut thread,
            CompactionStrategy::Summarize { keep_recent: 2 },
            None,
        )
        .await
        .expect("compact with summary should succeed");

    // Should keep only 2 recent turns
    assert_eq!(thread.turns.len(), 2);

    // The kept turns should be the last two (msg-3, msg-4)
    assert_eq!(thread.turns[0].user_input, "msg-3");
    assert_eq!(thread.turns[1].user_input, "msg-4");

    // Result should report the summary
    assert_eq!(result.turns_removed, 3);
    assert!(result.summary.is_some());
    let summary = result.summary.unwrap();
    assert!(summary.contains("User greeted the agent"));
    assert!(summary.contains("Five exchanges completed"));

    // summary_written should be false since no workspace was provided
    assert!(!result.summary_written);

    // StubLlm should have been called exactly once for the summary
    assert_eq!(llm.calls(), 1);
}

// ------------------------------------------------------------------
// 5. compact_with_summary: LLM failure returns error (does not corrupt thread)
// ------------------------------------------------------------------

#[tokio::test]
async fn test_compact_with_summary_llm_failure() {
    let llm = Arc::new(StubLlm::failing("broken-llm"));
    let compactor = make_compactor(llm.clone());
    let mut thread = make_thread(8);
    let original_len = thread.turns.len();

    let result = compactor
        .compact(
            &mut thread,
            CompactionStrategy::Summarize { keep_recent: 3 },
            None,
        )
        .await;

    // The LLM failure should propagate as an error
    assert!(result.is_err());

    // The thread should NOT have been modified (turns not truncated
    // on failure, since the error occurs before truncation)
    assert_eq!(thread.turns.len(), original_len);
}

// ------------------------------------------------------------------
// 6. compact_with_summary: fewer turns than keep_recent is a no-op
// ------------------------------------------------------------------

#[tokio::test]
async fn test_compact_with_summary_fewer_turns_than_keep() {
    let llm = Arc::new(StubLlm::new("should not be called"));
    let compactor = make_compactor(llm.clone());
    let mut thread = make_thread(3);

    let result = compactor
        .compact(
            &mut thread,
            CompactionStrategy::Summarize { keep_recent: 5 },
            None,
        )
        .await
        .expect("compact should succeed");

    // No turns removed, LLM never called
    assert_eq!(thread.turns.len(), 3);
    assert_eq!(result.turns_removed, 0);
    assert!(result.summary.is_none());
    assert_eq!(llm.calls(), 0);
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn test_compact_with_summary_preserves_turns_when_workspace_write_fails() {
    let llm = Arc::new(StubLlm::new("summary"));
    let compactor = make_compactor(llm.clone());
    let mut thread = make_thread(8);
    let original_inputs: Vec<String> = thread.turns.iter().map(|t| t.user_input.clone()).collect();
    let workspace = super::make_unmigrated_workspace()
        .await
        .expect("should create in-memory libsql backend");

    let result = compactor
        .compact(
            &mut thread,
            CompactionStrategy::Summarize { keep_recent: 3 },
            Some(&workspace),
        )
        .await
        .expect("compact should succeed even when workspace write fails");

    // On archival failure, no turns should be removed.
    assert_eq!(thread.turns.len(), 8);
    assert_eq!(
        thread
            .turns
            .iter()
            .map(|t| t.user_input.as_str())
            .collect::<Vec<_>>(),
        original_inputs
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(result.turns_removed, 0);
    assert!(!result.summary_written);
    assert_eq!(llm.calls(), 1);
}

// ------------------------------------------------------------------
// 14. Summarize with keep_recent=0 summarizes all and removes all
// ------------------------------------------------------------------

#[tokio::test]
async fn test_compact_with_summary_keep_zero() {
    let llm = Arc::new(StubLlm::new("Summary of all turns"));
    let compactor = make_compactor(llm.clone());
    let mut thread = make_thread(5);

    let result = compactor
        .compact(
            &mut thread,
            CompactionStrategy::Summarize { keep_recent: 0 },
            None,
        )
        .await
        .expect("compact should succeed");

    assert!(thread.turns.is_empty());
    assert_eq!(result.turns_removed, 5);
    assert!(result.summary.is_some());
    assert_eq!(result.summary.unwrap(), "Summary of all turns");
    assert_eq!(llm.calls(), 1);
}
