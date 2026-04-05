//! E2E tests: heartbeat runner.
//!
//! Tests that the HeartbeatRunner correctly processes heartbeat checklists
//! and handles findings or skips appropriately.

use std::sync::Arc;

use ironclaw::agent::{HeartbeatConfig, HeartbeatRunner};
use ironclaw::workspace::hygiene::HygieneConfig;

use crate::support::routines::{create_test_db, create_workspace};
use crate::support::trace_llm::{LlmTrace, TraceLlm, TraceResponse, TraceStep};

#[tokio::test]
async fn heartbeat_findings() {
    let (db, _tmp) = create_test_db().await;
    let ws = create_workspace(&db);

    // Write a real heartbeat checklist.
    ws.write(
        "HEARTBEAT.md",
        "# Heartbeat Checklist\n\n- [ ] Check if the server is running\n- [ ] Review error logs",
    )
    .await
    .expect("write heartbeat");

    // LLM responds with findings (not HEARTBEAT_OK).
    let trace = LlmTrace::single_turn(
        "test-heartbeat-findings",
        "heartbeat",
        vec![TraceStep {
            request_hint: None,
            response: TraceResponse::Text {
                content: "The server has elevated error rates. Review the logs immediately."
                    .to_string(),
                input_tokens: 100,
                output_tokens: 20,
            },
            expected_tool_results: vec![],
        }],
    );
    let llm = Arc::new(TraceLlm::from_trace(trace));

    let (tx, mut rx) = tokio::sync::mpsc::channel(16);

    let hygiene_config = HygieneConfig {
        enabled: false,
        daily_retention_days: 30,
        conversation_retention_days: 7,
        cadence_hours: 24,
        state_dir: _tmp.path().to_path_buf(),
    };

    let runner = HeartbeatRunner::new(HeartbeatConfig::default(), hygiene_config, ws, llm)
        .with_response_channel(tx);

    let result = runner.check_heartbeat().await;
    match result {
        ironclaw::agent::HeartbeatResult::NeedsAttention(msg) => {
            assert!(
                msg.contains("error"),
                "Expected 'error' in attention message: {msg}"
            );
        }
        other => panic!("Expected NeedsAttention, got: {other:?}"),
    }

    // No notification since we called check_heartbeat directly (not run).
    let _ = rx.try_recv();
}

#[tokio::test]
async fn heartbeat_empty_skip() {
    let (_db, _tmp) = create_test_db().await;
    let ws = create_workspace(&_db);

    // Write an effectively empty heartbeat (just headers and comments).
    ws.write(
        "HEARTBEAT.md",
        "# Heartbeat Checklist\n\n<!-- No tasks yet -->\n",
    )
    .await
    .expect("write heartbeat");

    // LLM should NOT be called, so provide a trace that would panic if called.
    let trace = LlmTrace::single_turn("test-heartbeat-skip", "skip", vec![]);
    let llm = Arc::new(TraceLlm::from_trace(trace));

    let hygiene_config = HygieneConfig {
        enabled: false,
        daily_retention_days: 30,
        conversation_retention_days: 7,
        cadence_hours: 24,
        state_dir: _tmp.path().to_path_buf(),
    };

    let runner = HeartbeatRunner::new(HeartbeatConfig::default(), hygiene_config, ws, llm);

    let result = runner.check_heartbeat().await;
    assert!(
        matches!(result, ironclaw::agent::HeartbeatResult::Skipped),
        "Expected Skipped for empty checklist, got: {result:?}"
    );
}
