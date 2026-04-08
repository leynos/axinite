//! Deterministic synchronisation helpers for routine engine E2E tests.
//!
//! Provides `wait_for_idle` and `wait_for_persisted_run` for coordinating
//! asynchronous routine execution and database persistence in tests that
//! trigger `RoutineEngine`'s task-spawning paths.
//!
//! This module is intentionally **not** declared in `support/mod.rs`.  It is
//! included directly from `tests/e2e_traces.rs` so that it is only compiled
//! into the test binary that actually calls these helpers, avoiding spurious
//! `dead_code` warnings in unrelated test binaries without requiring any lint
//! suppression.

#![cfg(feature = "libsql")]

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use uuid::Uuid;

use ironclaw::agent::routine_engine::RoutineEngine;
use ironclaw::db::Database;

/// Polls until the engine's running count reaches zero or the timeout expires.
///
/// This provides deterministic synchronisation for tests that need to wait
/// for asynchronous routine execution to complete, eliminating timing-dependent
/// flakiness without slowing down the test suite on fast machines.
///
/// **Note:** Combine with [`wait_for_persisted_run`] to ensure both execution
/// completion and database persistence, as the running count may reach zero
/// before the database record is fully committed.
pub async fn wait_for_idle(engine: &RoutineEngine, timeout: Duration) {
    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(10);

    loop {
        let count = engine.running_count().load(Ordering::SeqCst);
        if count == 0 {
            return;
        }

        if start.elapsed() >= timeout {
            panic!(
                "Timeout waiting for engine to become idle (running count: {})",
                count
            );
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// Polls until a routine run is persisted in the database or the timeout expires.
///
/// This helper provides deterministic synchronisation for database persistence,
/// complementing [`wait_for_idle`] which only waits for in-memory execution
/// completion.  Call this after `wait_for_idle` to ensure the routine run is
/// durably recorded before asserting on persisted state.
///
/// # Arguments
/// * `db` - The database to query for persisted runs.
/// * `routine_id` - The ID of the routine to check for runs.
/// * `timeout` - Maximum duration to wait for persistence.
pub async fn wait_for_persisted_run(db: &Arc<dyn Database>, routine_id: Uuid, timeout: Duration) {
    let start = std::time::Instant::now();
    let poll_interval = Duration::from_millis(10);
    let max_attempts: u32 = 500; // At 10ms intervals, this is ~5 seconds.

    let mut attempts: u32 = 0;
    loop {
        let runs = db
            .list_routine_runs(routine_id, 10)
            .await
            .expect("list_routine_runs should not fail");

        if !runs.is_empty() {
            return;
        }

        attempts += 1;
        if attempts >= max_attempts || start.elapsed() >= timeout {
            panic!(
                "Timeout waiting for routine run to be persisted (routine_id: {routine_id}, attempts: {attempts})"
            );
        }

        tokio::time::sleep(poll_interval).await;
    }
}