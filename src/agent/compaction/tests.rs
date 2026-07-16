//! Unit tests for thread turn formatting and compaction strategies.
//!
//! Shared helpers live here; the strategy-specific cases are grouped into
//! themed submodules.

use std::sync::Arc;

use uuid::Uuid;

use crate::agent::session::Thread;
use crate::testing::StubLlm;

use super::ContextCompactor;

mod formatting;
mod summarize;
mod truncate;
mod workspace;

/// Helper: build a `ContextCompactor` with the given `StubLlm`.
fn make_compactor(llm: Arc<StubLlm>) -> ContextCompactor {
    ContextCompactor::new(llm)
}

/// Helper: build a thread with `n` completed turns.
/// Turn `i` has user_input "msg-{i}" and response "resp-{i}".
fn make_thread(n: usize) -> Thread {
    let mut thread = Thread::new(Uuid::new_v4());
    for i in 0..n {
        thread.start_turn(format!("msg-{}", i));
        thread.complete_turn(format!("resp-{}", i));
    }
    thread
}

#[cfg(feature = "libsql")]
async fn make_unmigrated_workspace()
-> Result<crate::workspace::Workspace, crate::error::DatabaseError> {
    use crate::db::Database;
    use crate::db::libsql::LibSqlBackend;

    // Intentionally skip migrations so workspace append operations fail.
    let backend = LibSqlBackend::new_memory().await?;
    let db: Arc<dyn Database> = Arc::new(backend);
    Ok(crate::workspace::Workspace::new_with_db(
        "compaction-test",
        db,
    ))
}
