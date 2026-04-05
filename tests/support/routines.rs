//! Shared test-support utilities for routine and heartbeat tests.
//!
//! Provides reusable helpers for creating test databases, workspaces, routines,
//! and engines used across routine-related E2E tests.

#![cfg(feature = "libsql")]
// These items are used in e2e_traces but not in other test binaries.
#![allow(dead_code)]

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
pub struct SystemEventSpec<'a> {
    pub source: &'a str,
    pub event_type: &'a str,
    pub payload: serde_json::Value,
}

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
pub async fn create_test_db() -> (Arc<dyn Database>, TempDir) {
    use ironclaw::db::libsql::LibSqlBackend;

    let temp_dir = tempfile::tempdir().expect("tempdir");
    let db_path = temp_dir.path().join("test.db");
    let backend = LibSqlBackend::new_local(&db_path)
        .await
        .expect("LibSqlBackend");
    backend.run_migrations().await.expect("migrations");
    let db: Arc<dyn Database> = Arc::new(backend);
    (db, temp_dir)
}

/// Create a workspace backed by the test database.
pub fn create_workspace(db: &Arc<dyn Database>) -> Arc<Workspace> {
    Arc::new(Workspace::new_with_db("default", db.clone()))
}

/// Helper to insert a routine directly into the database.
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

/// Build a minimal RoutineEngine from a TraceLlm.
pub fn make_minimal_engine(
    trace: LlmTrace,
    db: Arc<dyn Database>,
    ws: Arc<Workspace>,
) -> Arc<RoutineEngine> {
    let llm = Arc::new(TraceLlm::from_trace(trace));
    let (notify_tx, _notify_rx) = tokio::sync::mpsc::channel(16);
    let tools = Arc::new(ToolRegistry::new());
    let safety = Arc::new(SafetyLayer::new(&SafetyConfig {
        max_output_length: 100_000,
        injection_check_enabled: true,
    }));
    Arc::new(RoutineEngine::new(
        RoutineConfig::default(),
        db,
        llm,
        ws,
        notify_tx,
        None,
        tools,
        safety,
    ))
}

/// Register a GitHub issue routine for system event tests.
pub async fn register_github_issue_routine(
    db: &Arc<dyn Database>,
    engine: &RoutineEngine,
) -> Routine {
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
    db.create_routine(&routine).await.expect("create_routine");
    engine.refresh_event_cache().await;
    routine
}

/// Assert that a system event fires the expected number of routines.
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
