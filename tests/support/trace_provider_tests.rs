//! Unit tests for trace-provider internals.

use std::panic;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;

use ironclaw::error::LlmError;
use ironclaw::llm::recording::{TraceResponse, TraceStep};
use ironclaw::llm::{ChatMessage, LlmProvider, ToolCompletionRequest};

use super::trace_provider::TraceLlm;
use super::trace_types::LlmTrace;

fn text_step(content: &str) -> TraceStep {
    TraceStep {
        request_hint: None,
        response: TraceResponse::Text {
            content: content.to_string(),
            input_tokens: 1,
            output_tokens: 1,
        },
        expected_tool_results: Vec::new(),
    }
}

#[test]
fn increment_hint_mismatches_panics_on_overflow() {
    let llm = TraceLlm::from_trace(LlmTrace::single_turn(
        "overflow-model",
        "hello",
        vec![text_step("hi")],
    ));
    llm.hint_mismatches.store(usize::MAX, Ordering::Relaxed);

    let result = panic::catch_unwind(|| llm.increment_hint_mismatches());

    let panic_payload = result.expect_err("expected hint mismatch overflow panic");
    let message = panic_payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic_payload.downcast_ref::<String>().map(String::as_str))
        .expect("panic payload should be a string");
    assert_eq!(message, "hint_mismatches overflowed");
}

#[tokio::test]
async fn poisoned_inner_lock_returns_request_failed() {
    let llm = Arc::new(TraceLlm::from_trace(LlmTrace::single_turn(
        "poison-model",
        "hello",
        vec![text_step("hi")],
    )));
    let poison_llm = Arc::clone(&llm);
    thread::spawn(move || {
        let _guard = poison_llm
            .inner
            .lock()
            .expect("TraceLlm state lock should open before poisoning");
        panic!("poison TraceLlm inner lock");
    })
    .join()
    .expect_err("poisoning thread should panic");

    let captured_err = llm
        .captured_requests()
        .expect_err("poisoned lock should reject diagnostics");
    assert!(
        matches!(captured_err, LlmError::RequestFailed { .. }),
        "expected request failure, got {captured_err:?}"
    );
    assert!(
        captured_err
            .to_string()
            .contains("TraceLlm state lock poisoned"),
        "expected poisoned-lock diagnostic, got {captured_err}"
    );

    let completion_err = llm
        .complete_with_tools(ToolCompletionRequest::new(
            vec![ChatMessage::user("hello")],
            vec![],
        ))
        .await
        .expect_err("poisoned lock should reject replay");
    assert!(
        matches!(completion_err, LlmError::RequestFailed { .. }),
        "expected request failure, got {completion_err:?}"
    );
    assert!(
        completion_err
            .to_string()
            .contains("TraceLlm state lock poisoned"),
        "expected poisoned-lock diagnostic, got {completion_err}"
    );
}

#[tokio::test]
async fn next_step_errors_on_cursor_overflow() {
    let llm = TraceLlm::from_trace(LlmTrace::single_turn(
        "overflow-model",
        "hello",
        vec![text_step("hi")],
    ));
    {
        let mut inner = llm.inner.lock().expect("TraceLlm state lock should open");
        inner.index = usize::MAX;
    }

    let err = llm
        .complete_with_tools(ToolCompletionRequest::new(
            vec![ChatMessage::user("hello")],
            vec![],
        ))
        .await
        .expect_err("cursor overflow should fail replay");

    assert!(
        matches!(err, LlmError::RequestFailed { .. }),
        "expected request failure, got {err:?}"
    );
    assert!(
        err.to_string().contains("overflowed"),
        "expected overflow diagnostic, got {err}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_calls_advance_cursor_monotonically() {
    let steps = (0..64)
        .map(|index| text_step(&format!("response {index}")))
        .collect();
    let llm = Arc::new(TraceLlm::from_trace(LlmTrace::single_turn(
        "concurrent-model",
        "hello",
        steps,
    )));

    let handles: Vec<_> = (0..8)
        .map(|index| {
            let llm = Arc::clone(&llm);
            tokio::spawn(async move {
                llm.complete_with_tools(ToolCompletionRequest::new(
                    vec![ChatMessage::user(format!("hello {index}"))],
                    vec![],
                ))
                .await
            })
        })
        .collect();

    let mut successes = 0;
    for handle in handles {
        let response = handle.await.expect("task should not panic");
        if response.is_ok() {
            successes += 1;
        }
    }

    assert_eq!(successes, 8);
    let final_cursor = llm
        .inner
        .lock()
        .expect("TraceLlm state lock should open")
        .index;
    assert_eq!(final_cursor, successes);
    assert_eq!(llm.hint_mismatches.load(Ordering::SeqCst), 0);
}
