//! E2E tests: cron-triggered routines.
//!
//! Tests that routines with cron schedules fire correctly when their
//! next_fire_at time is in the past.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;

use ironclaw::agent::routine::Trigger;
use ironclaw::agent::routine_engine::RoutineEngine;
use ironclaw::config::{RoutineConfig, SafetyConfig};
use ironclaw::safety::SafetyLayer;
use ironclaw::tools::ToolRegistry;

use crate::support::routines::{create_test_db, create_workspace, make_routine};
use crate::support::trace_llm::{LlmTrace, TraceLlm, TraceResponse, TraceStep};

#[tokio::test]
async fn cron_routine_fires() {
    let (db, _tmp) = create_test_db().await;
    let ws = create_workspace(&db);

    // Create a TraceLlm that responds with ROUTINE_OK.
    let trace = LlmTrace::single_turn(
        "test-cron-fire",
        "check",
        vec![TraceStep {
            request_hint: None,
            response: TraceResponse::Text {
                content: "ROUTINE_OK".to_string(),
                input_tokens: 50,
                output_tokens: 5,
            },
            expected_tool_results: vec![],
        }],
    );
    let llm = Arc::new(TraceLlm::from_trace(trace));

    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::channel(16);

    // Create minimal ToolRegistry and SafetyLayer for test.
    let tools = Arc::new(ToolRegistry::new());
    let safety_config = SafetyConfig {
        max_output_length: 100_000,
        injection_check_enabled: true,
    };
    let safety = Arc::new(SafetyLayer::new(&safety_config));

    let engine = Arc::new(RoutineEngine::new(
        RoutineConfig::default(),
        db.clone(),
        llm,
        ws,
        notify_tx,
        None,
        tools,
        safety,
    ));

    // Insert a cron routine with next_fire_at in the past.
    let mut routine = make_routine(
        "cron-test",
        Trigger::Cron {
            schedule: "* * * * *".to_string(),
            timezone: None,
        },
        "Check system status.",
    );
    routine.next_fire_at = Some(Utc::now() - chrono::Duration::minutes(5));
    db.create_routine(&routine).await.expect("create_routine");

    // Fire cron triggers.
    engine.check_cron_triggers().await;

    // Give the spawned task time to execute.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify a run was recorded.
    let runs = db
        .list_routine_runs(routine.id, 10)
        .await
        .expect("list_routine_runs");
    assert!(
        !runs.is_empty(),
        "Expected at least one routine run after cron trigger"
    );

    // Notification may or may not be sent depending on config;
    // just verify no panic occurred. Drain the channel.
    let _ = notify_rx.try_recv();
}
