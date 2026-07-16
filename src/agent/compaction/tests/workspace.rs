//! Tests for the `MoveToWorkspace` compaction strategy.

use std::sync::Arc;

use crate::agent::context_monitor::CompactionStrategy;
use crate::testing::StubLlm;

use super::{make_compactor, make_thread};

// ------------------------------------------------------------------
// 7. compact_to_workspace without workspace falls back to truncation
// ------------------------------------------------------------------

#[tokio::test]
async fn test_compact_to_workspace_without_workspace_falls_back() {
    let llm = Arc::new(StubLlm::new("unused"));
    let compactor = make_compactor(llm);
    let mut thread = make_thread(20);

    let result = compactor
        .compact(&mut thread, CompactionStrategy::MoveToWorkspace, None)
        .await
        .expect("compact should succeed");

    // Without a workspace, compact_to_workspace falls back to truncation
    // keeping 5 turns (the hardcoded fallback in the code)
    assert_eq!(thread.turns.len(), 5);
    assert_eq!(result.turns_removed, 15);

    // The remaining turns should be the last 5
    assert_eq!(thread.turns[0].user_input, "msg-15");
    assert_eq!(thread.turns[4].user_input, "msg-19");
}

// ------------------------------------------------------------------
// 8. compact_to_workspace: fewer turns than keep is a no-op
// ------------------------------------------------------------------

#[tokio::test]
async fn test_compact_to_workspace_fewer_turns_noop() {
    let llm = Arc::new(StubLlm::new("unused"));
    let compactor = make_compactor(llm);
    // MoveToWorkspace keeps 10 turns when workspace is available.
    // Without workspace it falls back to truncate(5).
    // With fewer turns, test the no-workspace fallback path:
    let mut thread = make_thread(4);

    let result = compactor
        .compact(&mut thread, CompactionStrategy::MoveToWorkspace, None)
        .await
        .expect("compact should succeed");

    // 4 turns < 5 (fallback keep_recent), so no truncation
    assert_eq!(thread.turns.len(), 4);
    assert_eq!(result.turns_removed, 0);
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn test_compact_to_workspace_preserves_turns_when_workspace_write_fails() {
    let llm = Arc::new(StubLlm::new("unused"));
    let compactor = make_compactor(llm.clone());
    let mut thread = make_thread(20);
    let original_inputs: Vec<String> = thread.turns.iter().map(|t| t.user_input.clone()).collect();
    let workspace = super::make_unmigrated_workspace()
        .await
        .expect("should create in-memory libsql backend");

    let result = compactor
        .compact(
            &mut thread,
            CompactionStrategy::MoveToWorkspace,
            Some(&workspace),
        )
        .await
        .expect("compact should succeed even when workspace write fails");

    // On archival failure, no turns should be removed.
    assert_eq!(thread.turns.len(), 20);
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
    assert_eq!(llm.calls(), 0);
}
