//! E2E spot-check tests adapted from nearai/benchmarks SpotSuite tasks.jsonl.
//!
//! Each test replays an LLM trace through the real agent loop and validates
//! the result using declarative `expects` from the fixture JSON plus any
//! additional assertions that can't be expressed declaratively.

use std::time::Duration;

use crate::support::cleanup::CleanupGuard;
use crate::support::test_rig::TestRigBuilder;
use crate::support::trace_llm::LlmTrace;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/llm_traces/spot"
);
const TIMEOUT: Duration = Duration::from_secs(15);

async fn run_spot_test(fixture_file: &str, message: &str) {
    let trace = LlmTrace::from_file(format!("{FIXTURES}/{fixture_file}")).unwrap();
    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .build()
        .await;

    rig.send_message(message).await;
    let responses = rig.wait_for_responses(1, TIMEOUT).await;

    rig.verify_trace_expects(&trace, &responses);
    rig.shutdown();
}

/// Generates a `#[tokio::test]` wrapper that delegates to `run_spot_test`.
macro_rules! spot_test {
    ($name:ident, $fixture:literal, $message:literal) => {
        #[tokio::test]
        async fn $name() {
            run_spot_test($fixture, $message).await;
        }
    };
}

// -----------------------------------------------------------------------
// Smoke tests -- no tools expected
// -----------------------------------------------------------------------

spot_test!(
    spot_smoke_greeting,
    "smoke_greeting.json",
    "Hello! Introduce yourself briefly."
);
spot_test!(
    spot_smoke_math,
    "smoke_math.json",
    "What is 47 * 23? Reply with just the number."
);

// -----------------------------------------------------------------------
// Tool tests -- verify correct tool selection
// -----------------------------------------------------------------------

spot_test!(
    spot_tool_echo,
    "tool_echo.json",
    "Use the echo tool to repeat the message: 'Spot check passed'"
);
spot_test!(
    spot_tool_json,
    "tool_json.json",
    "Parse this json for me: {\"key\": \"value\"}"
);

// -----------------------------------------------------------------------
// Chain tests -- multi-tool sequences
// -----------------------------------------------------------------------

#[tokio::test]
async fn spot_chain_write_read() {
    let _cleanup = CleanupGuard::new().file("/tmp/ironclaw_spot_test.txt");
    let _ = std::fs::remove_file("/tmp/ironclaw_spot_test.txt");

    let trace = LlmTrace::from_file(format!("{FIXTURES}/chain_write_read.json")).unwrap();
    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .build()
        .await;

    rig.send_message(
        "Write the text 'ironclaw spot check' to /tmp/ironclaw_spot_test.txt \
         using the write_file tool, then read it back using read_file.",
    )
    .await;
    let responses = rig.wait_for_responses(1, TIMEOUT).await;

    rig.verify_trace_expects(&trace, &responses);

    // Extra: verify file on disk (can't express in expects).
    let content =
        std::fs::read_to_string("/tmp/ironclaw_spot_test.txt").expect("file should exist");
    assert_eq!(content, "ironclaw spot check");

    rig.shutdown();
}

// -----------------------------------------------------------------------
// Robustness tests -- correct behavior under constraints
// -----------------------------------------------------------------------

spot_test!(
    spot_robust_no_tool,
    "robust_no_tool.json",
    "What is the capital of France? Answer directly without using any tools."
);
spot_test!(
    spot_robust_correct_tool,
    "robust_correct_tool.json",
    "Please echo the word 'deterministic output'"
);

// -----------------------------------------------------------------------
// Memory tests -- save and recall via file tools
// -----------------------------------------------------------------------

#[tokio::test]
async fn spot_memory_save_recall() {
    let _cleanup = CleanupGuard::new().file("/tmp/bench-meeting.md");
    let _ = std::fs::remove_file("/tmp/bench-meeting.md");

    let trace = LlmTrace::from_file(format!("{FIXTURES}/memory_save_recall.json")).unwrap();
    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .build()
        .await;

    rig.send_message(
        "Save these meeting notes to /tmp/bench-meeting.md:\n\
         Meeting: Project Phoenix sync\nAttendees: Alice, Bob, Carol\n\
         Decisions:\n- Launch date: April 15th\n- Budget: $50k approved\n\
         - Bob owns frontend, Carol owns backend\n\
         Then read it back and tell me who owns the frontend and what the launch date is.",
    )
    .await;
    let responses = rig.wait_for_responses(1, TIMEOUT).await;

    rig.verify_trace_expects(&trace, &responses);
    rig.shutdown();
}
