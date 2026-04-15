//! E2E tests: cron-triggered routines.
//!
//! Tests that routines with cron schedules fire correctly when their
//! next_fire_at time is in the past.

use std::time::Duration;

use chrono::Utc;

use ironclaw::agent::routine::Trigger;

use crate::support::routines::engine_sync::{wait_for_idle, wait_for_persisted_run};
use crate::support::routines::{
    create_test_db, create_workspace, make_minimal_engine, make_routine,
};
use crate::support::trace_llm::{LlmTrace, TraceResponse, TraceStep};

#[tokio::test]
async fn cron_routine_fires() -> anyhow::Result<()> {
    let (db, _tmp) = create_test_db().await.expect("create_test_db");
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
    let (engine, mut notify_rx) = make_minimal_engine(trace, db.clone(), ws);

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

    // Wait for routine execution to complete using deterministic synchronization,
    // then verify the routine run was recorded.
    wait_for_idle(&engine, Duration::from_secs(5)).await?;

    // Wait for routine run to be durably persisted in the database.
    // Snapshot run count before firing (zero for a freshly-created routine).
    wait_for_persisted_run(&db, routine.id, 0, Duration::from_secs(5)).await?;

    // Notification may or may not be sent depending on config;
    // just verify no panic occurred. Drain the channel.
    let _ = notify_rx.try_recv();

    Ok(())
}
