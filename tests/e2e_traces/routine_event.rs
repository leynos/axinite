//! E2E tests: event-triggered routines.
//!
//! Tests that routines with event triggers fire correctly when incoming
//! messages match the configured pattern.

use ironclaw::agent::routine::Trigger;

use crate::support::routines::{
    create_test_db, create_workspace, make_minimal_engine, make_routine, make_test_incoming_message,
};
use crate::support::trace_llm::{LlmTrace, TraceResponse, TraceStep};

#[tokio::test]
async fn event_trigger_matches() {
    let (db, _tmp) = create_test_db().await;
    let ws = create_workspace(&db);

    let trace = LlmTrace::single_turn(
        "test-event-match",
        "deploy",
        vec![TraceStep {
            request_hint: None,
            response: TraceResponse::Text {
                content: "Deployment detected".to_string(),
                input_tokens: 50,
                output_tokens: 10,
            },
            expected_tool_results: vec![],
        }],
    );
    let engine = make_minimal_engine(trace, db.clone(), ws);

    // Insert an event routine matching "deploy.*production".
    let routine = make_routine(
        "deploy-watcher",
        Trigger::Event {
            channel: None,
            pattern: "deploy.*production".to_string(),
        },
        "Report on deployment.",
    );
    db.create_routine(&routine).await.expect("create_routine");

    // Refresh the event cache so the engine knows about the routine.
    engine.refresh_event_cache().await;

    // Positive match: message containing "deploy to production".
    let matching_msg = make_test_incoming_message("deploy to production now");
    let fired = engine.check_event_triggers(&matching_msg).await;
    assert!(
        fired >= 1,
        "Expected >= 1 routine fired on match, got {fired}"
    );

    // Give spawn time.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Negative match: message that doesn't match.
    let non_matching_msg = make_test_incoming_message("check the staging environment");
    let fired_neg = engine.check_event_triggers(&non_matching_msg).await;
    assert_eq!(fired_neg, 0, "Expected 0 routines fired on non-match");
}
