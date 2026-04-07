//! E2E tests: system-event-triggered routines.
//!
//! Tests that routines with system event triggers fire correctly when
//! system events match the configured source, event type, and filters.

use crate::support::routines::{
    SystemEventSpec, assert_system_event_count, create_test_db, create_workspace,
    make_minimal_engine, register_github_issue_routine,
};
use crate::support::trace_llm::{LlmTrace, TraceResponse, TraceStep};

#[tokio::test]
async fn system_event_trigger_matches_and_filters() {
    let (db, _tmp) = create_test_db().await.expect("create_test_db");
    let ws = create_workspace(&db);
    let trace = LlmTrace::single_turn(
        "test-system-event-match",
        "event",
        vec![TraceStep {
            request_hint: None,
            response: TraceResponse::Text {
                content: "System event handled".to_string(),
                input_tokens: 40,
                output_tokens: 8,
            },
            expected_tool_results: vec![],
        }],
    );
    let (engine, _notify_rx) = make_minimal_engine(trace, db.clone(), ws);
    let routine = register_github_issue_routine(&db, &engine).await;

    // Matching event should fire and be recorded in run history.
    assert_system_event_count(
        &engine,
        SystemEventSpec::new(
            "github",
            "issue.opened",
            serde_json::json!({"repository": "nearai/ironclaw", "issue_number": 42}),
        ),
        1,
        "Expected one routine to fire for matching event",
    )
    .await;

    // Poll for routine completion with timeout.
    let mut attempts = 0;
    let max_attempts = 50;
    loop {
        let runs = db
            .list_routine_runs(routine.id, 10)
            .await
            .expect("list runs");
        if !runs.is_empty() {
            break;
        }
        attempts += 1;
        assert!(
            attempts < max_attempts,
            "Routine did not complete within timeout"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    // Table-driven checks for non-matching and case-insensitive scenarios.
    #[rustfmt::skip]
    let scenarios: Vec<(SystemEventSpec, usize, &str)> = vec![
        (SystemEventSpec::new("github", "issue.closed", serde_json::json!({"repository": "nearai/ironclaw"})),
         0, "Expected no routine for wrong event type"),
        (SystemEventSpec::new("github", "issue.opened", serde_json::json!({"repository": "other/repo"})),
         0, "Expected no routine for filter mismatch"),
        (SystemEventSpec::new("GitHub", "Issue.Opened", serde_json::json!({"repository": "nearai/ironclaw", "issue_number": 99})),
         1, "Expected case-insensitive match on source/event_type"),
        (SystemEventSpec::new("github", "issue.opened", serde_json::json!({"repository": "NearAI/IronClaw"})),
         1, "Expected case-insensitive match on filter values"),
    ];
    for (spec, expected, msg) in scenarios {
        assert_system_event_count(&engine, spec, expected, msg).await;
    }
}
