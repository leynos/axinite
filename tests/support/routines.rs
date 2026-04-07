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
#[expect(
    dead_code,
    reason = "public test-support struct used across feature-gated E2E test modules; \
              the compiler cannot see cross-module test usage"
)]
pub struct SystemEventSpec<'a> {
    pub source: &'a str,
    pub event_type: &'a str,
    pub payload: serde_json::Value,
}

#[expect(
    dead_code,
    reason = "public test-support constructor used across feature-gated E2E test modules; \
              the compiler cannot see cross-module test usage"
)]
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
#[expect(
    dead_code,
    reason = "public test-support helper used across feature-gated E2E test modules; \
              the compiler cannot see cross-module test usage"
)]
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
#[expect(
    dead_code,
    reason = "public test-support helper used across feature-gated E2E test modules; \
              the compiler cannot see cross-module test usage"
)]
pub fn create_workspace(db: &Arc<dyn Database>) -> Arc<Workspace> {
    Arc::new(Workspace::new_with_db("default", db.clone()))
}

/// Helper to insert a routine directly into the database.
#[expect(
    dead_code,
    reason = "public test-support helper used across feature-gated E2E test modules; \
              the compiler cannot see cross-module test usage"
)]
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
#[expect(
    dead_code,
    reason = "public test-support helper used across feature-gated E2E test modules; \
              the compiler cannot see cross-module test usage"
)]
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
#[expect(
    dead_code,
    reason = "public test-support helper used across feature-gated E2E test modules; \
              the compiler cannot see cross-module test usage"
)]
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
#[expect(
    dead_code,
    reason = "public test-support helper used across feature-gated E2E test modules; \
              the compiler cannot see cross-module test usage"
)]
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
#[expect(
    dead_code,
    reason = "public test-support helper used across feature-gated E2E test modules; \
              the compiler cannot see cross-module test usage"
)]
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

#[cfg(test)]
mod tests {
    use ironclaw::agent::routine::Trigger;

    use super::{SystemEventSpec, create_test_db, create_workspace, make_routine,
                make_test_incoming_message};

    #[test]
    fn make_routine_sets_fields() {
        let trigger = Trigger::Cron {
            schedule: "* * * * *".to_string(),
            timezone: None,
        };
        let routine = make_routine("my-routine", trigger, "Do the thing.");

        assert_eq!(routine.name, "my-routine", "name should match argument");
        assert!(
            routine.description.contains("my-routine"),
            "description should reference the routine name"
        );
        assert_eq!(
            routine.user_id, "default",
            "user_id should be 'default' for test routines"
        );
        assert!(routine.enabled, "test routines should be enabled");
        assert_eq!(
            routine.run_count, 0,
            "run_count should start at zero"
        );
        assert!(routine.last_run_at.is_none(), "last_run_at should be None");
        assert!(routine.next_fire_at.is_none(), "next_fire_at should be None");

        match &routine.action {
            ironclaw::agent::routine::RoutineAction::Lightweight { prompt, .. } => {
                assert_eq!(prompt, "Do the thing.", "action prompt should match argument");
            }
        }
    }

    #[test]
    fn make_test_incoming_message_sets_content() {
        let msg = make_test_incoming_message("hello world");

        assert_eq!(
            msg.content, "hello world",
            "content should match argument"
        );
        assert_eq!(
            msg.channel, "test",
            "channel should be 'test' for test messages"
        );
        assert_eq!(
            msg.user_id, "default",
            "user_id should be 'default' for test messages"
        );
        assert!(msg.user_name.is_none(), "user_name should be None");
        assert!(msg.thread_id.is_none(), "thread_id should be None");
        assert!(msg.attachments.is_empty(), "attachments should be empty");
    }

    #[test]
    fn system_event_spec_new_stores_fields() {
        let payload = serde_json::json!({"key": "value"});
        let spec = SystemEventSpec::new("github", "issue.opened", payload.clone());

        assert_eq!(spec.source, "github", "source should match argument");
        assert_eq!(
            spec.event_type, "issue.opened",
            "event_type should match argument"
        );
        assert_eq!(spec.payload, payload, "payload should match argument");
    }

    #[tokio::test]
    async fn create_test_db_returns_usable_database() {
        let (db, _tmp) = create_test_db()
            .await
            .expect("create_test_db should succeed");

        // Verify the database is functional by listing routines (empty result is fine).
        let routines = db.list_routines("default").await;
        assert!(
            routines.is_ok(),
            "database should be usable after creation: {routines:?}"
        );
    }

    #[tokio::test]
    async fn create_workspace_returns_workspace_with_correct_name() {
        let (db, _tmp) = create_test_db()
            .await
            .expect("create_test_db should succeed");
        let ws = create_workspace(&db);

        assert_eq!(
            ws.name(),
            "default",
            "workspace name should be 'default'"
        );
    }
}