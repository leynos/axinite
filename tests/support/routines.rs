//! Shared test-support utilities for routine and heartbeat tests.
//!
//! Provides reusable helpers for creating test databases, workspaces, routines,
//! and engines used across routine-related E2E tests.

#![cfg(feature = "libsql")]

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tempfile::TempDir;
use uuid::Uuid;

use ironclaw::agent::routine::{NotifyConfig, Routine, RoutineAction, RoutineGuardrails, Trigger};
use ironclaw::agent::routine_engine::RoutineEngine;
use ironclaw::channels::IncomingMessage;
use ironclaw::config::{RoutineConfig, SafetyConfig};
use ironclaw::db::Database;
use ironclaw::safety::SafetyLayer;
use ironclaw::tools::ToolRegistry;
use ironclaw::workspace::Workspace;

use crate::support::trace_llm::{LlmTrace, TraceLlm};

/// Describes a system event to be emitted in tests.
#[allow(dead_code)]
pub struct SystemEventSpec<'a> {
    pub source: &'a str,
    pub event_type: &'a str,
    pub payload: serde_json::Value,
}

#[allow(dead_code)]
impl<'a> SystemEventSpec<'a> {
    pub fn new(source: &'a str, event_type: &'a str, payload: serde_json::Value) -> Self {
        Self {
            source,
            event_type,
            payload,
        }
    }
}

/// Create a temp libSQL database with migrations applied.
#[allow(dead_code)]
pub async fn create_test_db() -> Result<(Arc<dyn Database>, TempDir), Box<dyn std::error::Error>> {
    use ironclaw::db::libsql::LibSqlBackend;

    let temp_dir = tempfile::tempdir()?;
    let db_path = temp_dir.path().join("test.db");
    let backend = LibSqlBackend::new_local(&db_path).await?;
    backend.run_migrations().await?;
    let db: Arc<dyn Database> = Arc::new(backend);
    Ok((db, temp_dir))
}

/// Create a workspace backed by the test database.
#[allow(dead_code)]
pub fn create_workspace(db: &Arc<dyn Database>) -> Arc<Workspace> {
    Arc::new(Workspace::new_with_db("default", db.clone()))
}

/// Helper to insert a routine directly into the database.
#[allow(dead_code)]
pub fn make_routine(name: &str, trigger: Trigger, prompt: &str) -> Routine {
    Routine {
        id: Uuid::new_v4(),
        name: name.to_string(),
        description: format!("Test routine: {name}"),
        user_id: "default".to_string(),
        enabled: true,
        trigger,
        action: RoutineAction::Lightweight {
            prompt: prompt.to_string(),
            context_paths: vec![],
            max_tokens: 1000,
        },
        guardrails: RoutineGuardrails {
            cooldown: Duration::from_secs(0),
            max_concurrent: 5,
            dedup_window: None,
        },
        notify: NotifyConfig::default(),
        last_run_at: None,
        next_fire_at: None,
        run_count: 0,
        consecutive_failures: 0,
        state: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

/// Build a minimal IncomingMessage for event-trigger tests.
#[allow(dead_code)]
pub fn make_test_incoming_message(content: &str) -> IncomingMessage {
    IncomingMessage {
        id: Uuid::new_v4(),
        channel: "test".to_string(),
        user_id: "default".to_string(),
        user_name: None,
        content: content.to_string(),
        thread_id: None,
        received_at: Utc::now(),
        metadata: serde_json::json!({}),
        timezone: None,
        attachments: Vec::new(),
    }
}

/// Build a minimal RoutineEngine from a TraceLlm, returning both the engine and the notify receiver.
#[allow(dead_code)]
pub fn make_minimal_engine(
    trace: LlmTrace,
    db: Arc<dyn Database>,
    ws: Arc<Workspace>,
) -> (
    Arc<RoutineEngine>,
    tokio::sync::mpsc::Receiver<ironclaw::channels::OutgoingResponse>,
) {
    let llm = Arc::new(TraceLlm::from_trace(trace));
    let (notify_tx, notify_rx) = tokio::sync::mpsc::channel(16);
    let tools = Arc::new(ToolRegistry::new());
    let safety = Arc::new(SafetyLayer::new(&SafetyConfig {
        max_output_length: 100_000,
        injection_check_enabled: true,
    }));
    let engine = Arc::new(RoutineEngine::new(
        RoutineConfig::default(),
        db,
        llm,
        ws,
        notify_tx,
        None,
        tools,
        safety,
    ));
    (engine, notify_rx)
}

/// Register a GitHub issue routine for system event tests.
#[allow(dead_code)]
pub async fn register_github_issue_routine(
    db: &Arc<dyn Database>,
    engine: &RoutineEngine,
) -> anyhow::Result<Routine> {
    let mut filters = std::collections::HashMap::new();
    filters.insert("repository".to_string(), "nearai/ironclaw".to_string());
    let routine = make_routine(
        "github-issue-opened",
        Trigger::SystemEvent {
            source: "github".to_string(),
            event_type: "issue.opened".to_string(),
            filters,
        },
        "Summarize the issue and propose an implementation plan.",
    );
    db.create_routine(&routine).await?;
    engine.refresh_event_cache().await;
    Ok(routine)
}

/// Assert that a system event fires the expected number of routines.
#[allow(dead_code)]
pub async fn assert_system_event_count(
    engine: &RoutineEngine,
    spec: SystemEventSpec<'_>,
    expected: usize,
    msg: &str,
) {
    let fired = engine
        .emit_system_event(spec.source, spec.event_type, &spec.payload, Some("default"))
        .await;
    assert_eq!(fired, expected, "{msg}");
}

/// Deterministic synchronization helpers for tests that drive [`RoutineEngine`].
///
/// Scoped into their own inline module so that test binaries which do not exercise
/// `RoutineEngine` (e.g. `heartbeat`) never reference these items, while
/// compile-time type assertions in `support::mod` prove liveness for the rest.
pub mod engine_sync {
    use std::sync::Arc;
    use std::time::Duration;

    use anyhow::anyhow;
    use uuid::Uuid;

    use ironclaw::agent::routine_engine::RoutineEngine;
    use ironclaw::db::Database;

    /// Waits briefly to let spawned routine work make progress before persistence checks.
    ///
    /// Integration tests do not compile against the `RoutineEngine::running_count`
    /// test-only hook unless `test-helpers` is enabled, so this helper provides
    /// a small best-effort hand-off point before [`wait_for_persisted_run`] does
    /// the durable synchronization.
    ///
    /// **Note:** Always combine with [`wait_for_persisted_run`] to ensure the
    /// database record is durably committed before asserting on stored state.
    pub async fn wait_for_idle(engine: &RoutineEngine, timeout: Duration) -> Result<(), anyhow::Error> {
        let _ = engine;
        tokio::time::sleep(timeout.min(Duration::from_millis(10))).await;
        Ok(())
    }

    /// Polls until a new routine run is persisted in the database or the timeout expires.
    ///
    /// Complements [`wait_for_idle`]: call this after `wait_for_idle` to ensure the
    /// routine run is durably recorded before asserting on database state.
    ///
    /// # Arguments
    /// * `db`                – The database to query for persisted runs.
    /// * `routine_id`        – The ID of the routine to check for runs.
    /// * `previous_run_count` – Snapshot of the run count before firing; the
    ///   function waits until the count exceeds this value.
    /// * `timeout`           – Maximum duration to wait for persistence.
    pub async fn wait_for_persisted_run(
        db: &Arc<dyn Database>,
        routine_id: Uuid,
        previous_run_count: usize,
        timeout: Duration,
    ) -> Result<(), anyhow::Error> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(10);

        loop {
            let runs = db
                .list_routine_runs(routine_id, 10)
                .await
                .map_err(|e| anyhow!(e))?;

            if runs.len() > previous_run_count {
                return Ok(());
            }

            if start.elapsed() >= timeout {
                return Err(anyhow!(
                    "Timeout waiting for routine run to be persisted (routine_id: {}, \
                     previous_count: {}, current_count: {}, elapsed: {:?})",
                    routine_id,
                    previous_run_count,
                    runs.len(),
                    start.elapsed()
                ));
            }

            tokio::time::sleep(poll_interval).await;
        }
    }
}
