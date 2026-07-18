//! End-to-end routine execution trace test: creates a news-digest
//! routine, fires it manually, and verifies the worker broadcasts the
//! digest, with recorded HTTP exchanges for the news API.

use super::*;

// -----------------------------------------------------------------------
// 6. Routine news digest (end-to-end: create, fire, verify message)
//
// Exercises the full routine execution stack:
//   routine_create → routine_fire → RoutineEngine::fire_manual →
//   Scheduler::dispatch_job_with_context → Worker (autonomous) →
//   http + memory_write + message (broadcast to test channel)
// -----------------------------------------------------------------------

fn build_news_api_http_exchanges() -> Vec<axinite::llm::recording::HttpExchange> {
    use axinite::llm::recording::{HttpExchange, HttpExchangeRequest, HttpExchangeResponse};
    vec![HttpExchange {
        request: HttpExchangeRequest {
            method: "GET".to_string(),
            url: "https://news-api.example.com/v1/tech/headlines".to_string(),
            headers: Vec::new(),
            body: None,
        },
        response: HttpExchangeResponse {
            status: 200,
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body: serde_json::json!({
                "headlines": [
                    {"title": "Rust 2026 Edition", "summary": "async closures, generator syntax"},
                    {"title": "WASM Component Model 1.0", "summary": "cross-language interop"},
                    {"title": "NEAR AI Agent Framework", "summary": "on-chain identity"}
                ]
            })
            .to_string(),
        },
    }]
}

fn assert_routine_created(responses: &[axinite::channels::OutgoingResponse]) {
    assert!(!responses.is_empty(), "Turn 1: no response");
    let t1 = responses[0].content.to_lowercase();
    assert!(
        t1.contains("routine") || t1.contains("created"),
        "Turn 1: expected routine/created, got: {t1}"
    );
}

#[tokio::test]
async fn routine_news_digest() {
    let trace = LlmTrace::from_file_async(fixture_path("advanced", "routine_news_digest.json"))
        .await
        .expect("failed to load fixture: advanced/routine_news_digest.json");

    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .with_routines()
        .with_http_exchanges(build_news_api_http_exchanges())
        .build()
        .await
        .expect("failed to build test rig");

    // Turn 1: Create the routine (manual trigger, full_job, message+http pre-authorized).
    rig.send_message(
        "Set up a morning tech news routine with manual trigger \
         and full_job mode. Pre-authorize the message and http tools.",
    )
    .await;
    let r1 = rig.wait_for_responses(1, LONG_TIMEOUT).await;
    assert_routine_created(&r1);

    // Turn 2: Fire the routine. This dispatches a full_job through the scheduler.
    // The routine worker runs asynchronously. We wait for at least the Turn 2 response
    // (bringing total to 2, including Turn 1), then poll for additional messages from the worker.
    rig.send_message("Fire it now.").await;
    let _turn2 = rig.wait_for_responses(2, Duration::from_secs(15)).await;

    // Poll for additional responses from the routine worker (timeout-based collection)
    let responses = rig.wait_for_responses(999, Duration::from_secs(10)).await;

    // Find the main conversation reply (from turn 2) by content, since
    // the routine worker runs asynchronously and may interleave messages.
    let fire_reply = responses.iter().find(|r| {
        let c = r.content.to_lowercase();
        c.contains("fired") || c.contains("running")
    });
    assert!(
        fire_reply.is_some(),
        "Turn 2: expected fired/running, got: {:?}",
        responses.iter().map(|r| &r.content).collect::<Vec<_>>()
    );

    // The routine worker runs autonomously: http → memory_write → message.
    // The message tool broadcasts to the test channel, proving the full
    // chain executed successfully (including ApprovalContext allowing the
    // http and message tools in autonomous mode).
    let message_broadcast = responses.iter().find(|r| {
        r.content.contains("Tech News Digest")
            || r.content.contains("Rust 2026")
            || r.content.contains("WASM Component Model")
    });
    assert!(
        message_broadcast.is_some(),
        "Routine worker should have broadcast a message. Got: {:?}",
        responses.iter().map(|r| &r.content).collect::<Vec<_>>()
    );

    // Verify main conversation tools were called.
    let started = rig.tool_calls_started();
    for tool in &["routine_create", "routine_fire"] {
        assert!(
            started.iter().any(|s| s == *tool),
            "{tool} not called: {started:?}"
        );
    }

    // Main conversation tools should have succeeded.
    let completed = rig.tool_calls_completed();
    crate::support::assertions::assert_all_tools_succeeded(&completed);

    rig.shutdown();
}
