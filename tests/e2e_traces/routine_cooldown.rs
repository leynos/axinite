//! E2E tests: routine cooldown behaviour.
//!
//! Tests that routines respect their configured cooldown period and
//! prevent re-triggering within the cooldown window.

use std::time::Duration;

use ironclaw::agent::routine::Trigger;

use crate::support::routines::engine_sync::{wait_for_idle, wait_for_persisted_run};
use crate::support::routines::{
    create_test_db, create_workspace, make_minimal_engine, make_routine, make_test_incoming_message,
};
use crate::support::trace_llm::{LlmTrace, TraceResponse, TraceStep};

#[tokio::test]
async fn routine_cooldown() {
    let (db, _tmp) = create_test_db().await.expect("create_test_db");
    let ws = create_workspace(&db);

    // Need two LLM responses (one for the first fire).
    let trace = LlmTrace::single_turn(
        "test-cooldown",
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
    let (engine, _notify_rx) = make_minimal_engine(trace, db.clone(), ws);

    // Insert an event routine with 1-hour cooldown.
    let mut routine = make_routine(
        "cooldown-test",
        Trigger::Event {
            channel: None,
            pattern: "test-cooldown".to_string(),
        },
        "Check status.",
    );
    routine.guardrails.cooldown = Duration::from_secs(3600);
    db.create_routine(&routine).await.expect("create_routine");
    engine.refresh_event_cache().await;

    // First fire should work.
    let msg = make_test_incoming_message("test-cooldown trigger");
    let fired1 = engine.check_event_triggers(&msg).await;
    assert!(fired1 >= 1, "First fire should work");

    // Wait for routine execution to complete using deterministic synchronization,
    // then verify the routine run was recorded in the database.
    wait_for_idle(&engine, Duration::from_secs(5)).await;

    // Wait for routine run to be durably persisted in the database.
    // Snapshot run count before firing (zero for a freshly-created routine).
    wait_for_persisted_run(&db, routine.id, 0, Duration::from_secs(5)).await;

    let persisted = db
        .get_routine(routine.id)
        .await
        .expect("get_routine")
        .expect("routine present");
    assert!(
        persisted.runtime.last_run_at.is_some(),
        "Expected engine to persist last_run_at"
    );
    assert!(
        persisted.runtime.run_count >= 1,
        "Expected engine to persist run_count"
    );

    // Second fire should be blocked by cooldown.
    let fired2 = engine.check_event_triggers(&msg).await;
    assert_eq!(fired2, 0, "Second fire should be blocked by cooldown");
}
